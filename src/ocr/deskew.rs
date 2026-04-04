//! Image deskewing using projection profile analysis.
//!
//! Detects text rotation angle and corrects it before OCR processing.

/// Maximum rotation angle to search for (degrees).
const MAX_ANGLE_DEG: f64 = 5.0;

/// Angle step size for search (degrees).
const ANGLE_STEP_DEG: f64 = 0.5;

/// Detect the skew angle of text in a grayscale image.
///
/// Uses projection profile variance: when text is aligned,
/// the horizontal projection has maximum variance (sharp peaks at text lines).
pub fn detect_skew_angle(gray_data: &[u8], width: u32, height: u32) -> f64 {
    let mut best_angle = 0.0;
    let mut best_variance = 0.0f64;

    let mut angle = -MAX_ANGLE_DEG;
    while angle <= MAX_ANGLE_DEG {
        let variance = projection_variance(gray_data, width as usize, height as usize, angle);
        if variance > best_variance {
            best_variance = variance;
            best_angle = angle;
        }
        angle += ANGLE_STEP_DEG;
    }

    best_angle
}

/// Rotate grayscale image data by the given angle (degrees).
///
/// Returns new pixel data with the same dimensions.
/// Rotation is around the image center, with white (255) fill for empty areas.
pub fn rotate_gray(data: &[u8], width: usize, height: usize, angle_deg: f64) -> Vec<u8> {
    if angle_deg.abs() < 0.1 {
        return data.to_vec();
    }

    let angle_rad = angle_deg.to_radians();
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();

    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;

    let mut result = vec![255u8; width * height];

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;

            let src_x = (dx * cos_a + dy * sin_a + cx).round() as isize;
            let src_y = (-dx * sin_a + dy * cos_a + cy).round() as isize;

            if src_x >= 0 && src_x < width as isize && src_y >= 0 && src_y < height as isize {
                let src_idx = (src_y as usize) * width + (src_x as usize);
                result[y * width + x] = data[src_idx];
            }
        }
    }

    result
}

/// Compute the variance of the horizontal projection profile at a given angle.
fn projection_variance(data: &[u8], width: usize, height: usize, angle_deg: f64) -> f64 {
    if angle_deg.abs() < 0.1 {
        return projection_variance_straight(data, width, height);
    }

    let angle_rad = angle_deg.to_radians();
    let sin_a = angle_rad.sin();
    let cos_a = angle_rad.cos();

    let cx = width as f64 / 2.0;
    let cy = height as f64 / 2.0;

    let diag = ((width * width + height * height) as f64).sqrt() as usize;
    let mut projection = vec![0.0f64; diag];

    for y in 0..height {
        for x in 0..width {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;

            let rotated_y = (-dx * sin_a + dy * cos_a + cy).round() as isize;

            if rotated_y >= 0 && rotated_y < diag as isize {
                let pixel_val = data[y * width + x] as f64;
                projection[rotated_y as usize] += 255.0 - pixel_val;
            }
        }
    }

    variance(&projection)
}

fn projection_variance_straight(data: &[u8], width: usize, height: usize) -> f64 {
    let mut projection = vec![0.0f64; height];

    for y in 0..height {
        for x in 0..width {
            let pixel_val = data[y * width + x] as f64;
            projection[y] += 255.0 - pixel_val;
        }
    }

    variance(&projection)
}

fn variance(values: &[f64]) -> f64 {
    let n = values.len() as f64;
    if n == 0.0 {
        return 0.0;
    }
    let mean: f64 = values.iter().sum::<f64>() / n;
    values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variance_uniform_values() {
        let values = vec![1.0, 1.0, 1.0, 1.0];
        assert!((variance(&values) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_variance_different_values() {
        let values = vec![0.0, 0.0, 255.0, 255.0];
        let var = variance(&values);
        assert!(var > 10000.0);
    }

    #[test]
    fn test_rotate_zero_degrees() {
        let data = vec![100u8, 200, 150, 50];
        let result = rotate_gray(&data, 2, 2, 0.0);
        assert_eq!(result, data);
    }

    #[test]
    fn test_detect_skew_angle_straight_text() {
        let mut data = vec![255u8; 100 * 100];
        for y in 20..30 {
            for x in 10..90 {
                data[y * 100 + x] = 0;
            }
        }
        for y in 50..60 {
            for x in 10..90 {
                data[y * 100 + x] = 0;
            }
        }

        let angle = detect_skew_angle(&data, 100, 100);
        assert!(angle.abs() < 1.0);
    }
}
