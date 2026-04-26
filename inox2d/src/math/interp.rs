use glam::Vec2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolateMode {
	/// Round to nearest
	Nearest,
	/// Linear interpolation
	Linear,
	// there's more but I'm not adding them for now.
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InterpRange<T> {
	pub beg: T,
	pub end: T,
}

impl<T> InterpRange<T> {
	#[inline]
	pub fn new(beg: T, end: T) -> Self {
		Self { beg, end }
	}
}

impl InterpRange<Vec2> {
	#[inline]
	pub fn to_x(self) -> InterpRange<f32> {
		InterpRange {
			beg: self.beg.x,
			end: self.end.x,
		}
	}

	#[inline]
	pub fn to_y(self) -> InterpRange<f32> {
		InterpRange {
			beg: self.beg.y,
			end: self.end.y,
		}
	}
}

#[inline]
fn interpolate_nearest(t: f32, range_in: InterpRange<f32>, range_out: InterpRange<f32>) -> f32 {
	debug_assert!(
		range_in.beg <= t && t <= range_in.end,
		"{} <= {} <= {}",
		range_in.beg,
		t,
		range_in.end
	);

	if range_in.beg == range_in.end {
		return range_out.beg;
	}

	if (range_in.end - t) < (t - range_in.beg) {
		range_out.end
	} else {
		range_out.beg
	}
}

#[inline]
fn interpolate_linear(t: f32, range_in: InterpRange<f32>, range_out: InterpRange<f32>) -> f32 {
	debug_assert!(
		range_in.beg <= t && t <= range_in.end,
		"{} is out of input range [{}, {}]",
		t,
		range_in.beg,
		range_in.end,
	);

	if range_in.beg == range_in.end {
		return range_out.beg;
	}

	(t - range_in.beg) * (range_out.end - range_out.beg) / (range_in.end - range_in.beg) + range_out.beg
}

#[inline]
pub fn interpolate_f32(t: f32, range_in: InterpRange<f32>, range_out: InterpRange<f32>, mode: InterpolateMode) -> f32 {
	match mode {
		InterpolateMode::Nearest => interpolate_nearest(t, range_in, range_out),
		InterpolateMode::Linear => interpolate_linear(t, range_in, range_out),
	}
}

#[inline]
pub fn interpolate_vec2(
	t: f32,
	range_in: InterpRange<f32>,
	range_out: InterpRange<Vec2>,
	mode: InterpolateMode,
) -> Vec2 {
	let x = interpolate_f32(t, range_in, range_out.to_x(), mode);
	let y = interpolate_f32(t, range_in, range_out.to_y(), mode);
	Vec2 { x, y }
}

pub fn interpolate_f32s_additive(
	t: f32,
	range_in: InterpRange<f32>,
	range_out: InterpRange<&[f32]>,
	mode: InterpolateMode,
	out: &mut [f32],
) {
	for ((&ob, &oe), o) in range_out.beg.iter().zip(range_out.end).zip(out) {
		*o += interpolate_f32(t, range_in, InterpRange::new(ob, oe), mode);
	}
}

pub fn interpolate_vec2s_additive(
	t: f32,
	range_in: InterpRange<f32>,
	range_out: InterpRange<&[Vec2]>,
	mode: InterpolateMode,
	out: &mut [Vec2],
) {
	for ((&ob, &oe), o) in range_out.beg.iter().zip(range_out.end).zip(out) {
		*o += interpolate_vec2(t, range_in, InterpRange::new(ob, oe), mode);
	}
}

#[inline]
pub fn bi_interpolate_f32(
	t: Vec2,
	range_in: InterpRange<Vec2>,
	out_top: InterpRange<f32>,
	out_bottom: InterpRange<f32>,
	mode: InterpolateMode,
) -> f32 {
	let beg = interpolate_f32(t.x, range_in.to_x(), out_top, mode);
	let end = interpolate_f32(t.x, range_in.to_x(), out_bottom, mode);
	interpolate_f32(t.y, range_in.to_y(), InterpRange::new(beg, end), mode)
}

#[inline]
pub fn bi_interpolate_vec2(
	t: Vec2,
	range_in: InterpRange<Vec2>,
	out_top: InterpRange<Vec2>,
	out_bottom: InterpRange<Vec2>,
	mode: InterpolateMode,
) -> Vec2 {
	let beg = interpolate_vec2(t.x, range_in.to_x(), out_top, mode);
	let end = interpolate_vec2(t.x, range_in.to_x(), out_bottom, mode);
	interpolate_vec2(t.y, range_in.to_y(), InterpRange::new(beg, end), mode)
}

pub fn bi_interpolate_f32s_additive(
	t: Vec2,
	range_in: InterpRange<Vec2>,
	out_top: InterpRange<&[f32]>,
	out_bottom: InterpRange<&[f32]>,
	mode: InterpolateMode,
	out: &mut [f32],
) {
	for (((&otb, &ote), (&obb, &obe)), o) in (out_top.beg.iter().zip(out_top.end))
		.zip(out_bottom.beg.iter().zip(out_bottom.end))
		.zip(out)
	{
		*o += bi_interpolate_f32(
			t,
			range_in,
			InterpRange::new(otb, ote),
			InterpRange::new(obb, obe),
			mode,
		)
	}
}

pub fn bi_interpolate_vec2s_additive(
	t: Vec2,
	range_in: InterpRange<Vec2>,
	out_top: InterpRange<&[Vec2]>,
	out_bottom: InterpRange<&[Vec2]>,
	mode: InterpolateMode,
	out: &mut [Vec2],
) {
	for (((&otb, &ote), (&obb, &obe)), o) in (out_top.beg.iter().zip(out_top.end))
		.zip(out_bottom.beg.iter().zip(out_bottom.end))
		.zip(out)
	{
		*o += bi_interpolate_vec2(
			t,
			range_in,
			InterpRange::new(otb, ote),
			InterpRange::new(obb, obe),
			mode,
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_linear_interpolation() {
		assert_eq!(
			interpolate_linear(0.0, InterpRange::new(0.0, 1.0), InterpRange::new(-5.0, 5.0)),
			-5.0
		);
		assert_eq!(
			interpolate_linear(1.0, InterpRange::new(0.0, 1.0), InterpRange::new(-5.0, 5.0)),
			5.0
		);
		assert_eq!(
			interpolate_linear(0.5, InterpRange::new(0.0, 1.0), InterpRange::new(-5.0, 5.0)),
			0.0
		);
		assert_eq!(
			interpolate_linear(0.0, InterpRange::new(-0.5, 0.0), InterpRange::new(-5.0, 5.0)),
			5.0
		);
	}

	#[test]
	fn test_degenerate_input_range() {
		assert_eq!(
			interpolate_linear(0.0, InterpRange::new(0.0, 0.0), InterpRange::new(3.0, 7.0)),
			3.0
		);
		assert_eq!(
			interpolate_nearest(0.0, InterpRange::new(0.0, 0.0), InterpRange::new(3.0, 7.0)),
			3.0
		);
	}

	#[test]
	fn test_bilinear_degenerate_input_range() {
		let mut out = vec![Vec2::ZERO; 2];
		let top_beg = vec![Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)];
		let top_end = vec![Vec2::new(5.0, 6.0), Vec2::new(7.0, 8.0)];
		let bottom_beg = vec![Vec2::new(9.0, 10.0), Vec2::new(11.0, 12.0)];
		let bottom_end = vec![Vec2::new(13.0, 14.0), Vec2::new(15.0, 16.0)];

		bi_interpolate_vec2s_additive(
			Vec2::ZERO,
			InterpRange::new(Vec2::ZERO, Vec2::ZERO),
			InterpRange::new(top_beg.as_slice(), top_end.as_slice()),
			InterpRange::new(bottom_beg.as_slice(), bottom_end.as_slice()),
			InterpolateMode::Linear,
			&mut out,
		);

		assert_eq!(out, top_beg);
	}
}
