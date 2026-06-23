#[cfg(test)]
mod tests {
    use crate::recording::stop_overlay::{
        display_bounds_from_monitor_geometries, monitor_index_for_capture_from_geometries,
        RecordingControlsParams,
    };

    #[test]
    fn display_bounds_handles_single_monitor_origin() {
        let bounds = display_bounds_from_monitor_geometries([(0, 0, 1920, 1080)]);
        assert_eq!(bounds, Some((0, 0, 1920, 1080)));
    }

    #[test]
    fn display_bounds_handles_monitor_to_the_right() {
        let bounds =
            display_bounds_from_monitor_geometries([(0, 0, 1920, 1080), (1920, 0, 2560, 1440)]);
        assert_eq!(bounds, Some((0, 0, 4480, 1440)));
    }

    #[test]
    fn display_bounds_handles_negative_monitor_origin() {
        let bounds =
            display_bounds_from_monitor_geometries([(-1600, 0, 1600, 900), (0, 0, 1920, 1080)]);
        assert_eq!(bounds, Some((-1600, 0, 3520, 1080)));
    }

    #[test]
    fn display_bounds_handles_monitor_above_primary() {
        let bounds =
            display_bounds_from_monitor_geometries([(0, -1024, 1280, 1024), (0, 0, 1920, 1080)]);
        assert_eq!(bounds, Some((0, -1024, 1920, 2104)));
    }

    #[test]
    fn monitor_index_uses_capture_center_on_primary() {
        let geometries = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        let params = RecordingControlsParams {
            capture_x: 200,
            capture_y: 150,
            capture_w: 600,
            capture_h: 400,
            ..Default::default()
        };
        assert_eq!(
            monitor_index_for_capture_from_geometries(&geometries, &params),
            Some(0)
        );
    }

    #[test]
    fn monitor_index_uses_capture_center_on_secondary_with_positive_offset() {
        let geometries = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        let params = RecordingControlsParams {
            capture_x: 2200,
            capture_y: 200,
            capture_w: 600,
            capture_h: 500,
            ..Default::default()
        };
        assert_eq!(
            monitor_index_for_capture_from_geometries(&geometries, &params),
            Some(1)
        );
    }

    #[test]
    fn monitor_index_uses_capture_center_on_secondary_with_negative_offset() {
        let geometries = [(-1600, 0, 1600, 900), (0, 0, 1920, 1080)];
        let params = RecordingControlsParams {
            capture_x: -1400,
            capture_y: 120,
            capture_w: 500,
            capture_h: 300,
            ..Default::default()
        };
        assert_eq!(
            monitor_index_for_capture_from_geometries(&geometries, &params),
            Some(0)
        );
    }

    #[test]
    fn monitor_index_returns_none_when_center_is_outside_all_monitors() {
        let geometries = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        let params = RecordingControlsParams {
            capture_x: 5000,
            capture_y: 5000,
            capture_w: 200,
            capture_h: 200,
            ..Default::default()
        };
        assert_eq!(
            monitor_index_for_capture_from_geometries(&geometries, &params),
            None
        );
    }
}
