use super::*;

#[test]
fn test_selection_normalize() {
    // Normal case (no normalization needed)
    let area = SelectionArea {
        x: 100,
        y: 100,
        width: 200,
        height: 150,
    };
    let normalized = area.normalize();
    assert_eq!(normalized.x, 100);
    assert_eq!(normalized.y, 100);
    assert_eq!(normalized.width, 200);
    assert_eq!(normalized.height, 150);

    // Negative width (dragged left)
    let area = SelectionArea {
        x: 300,
        y: 100,
        width: -200,
        height: 150,
    };
    let normalized = area.normalize();
    assert_eq!(normalized.x, 100);
    assert_eq!(normalized.y, 100);
    assert_eq!(normalized.width, 200);
    assert_eq!(normalized.height, 150);

    // Negative height (dragged up)
    let area = SelectionArea {
        x: 100,
        y: 250,
        width: 200,
        height: -150,
    };
    let normalized = area.normalize();
    assert_eq!(normalized.x, 100);
    assert_eq!(normalized.y, 100);
    assert_eq!(normalized.width, 200);
    assert_eq!(normalized.height, 150);

    // Both negative (dragged up-left)
    let area = SelectionArea {
        x: 300,
        y: 250,
        width: -200,
        height: -150,
    };
    let normalized = area.normalize();
    assert_eq!(normalized.x, 100);
    assert_eq!(normalized.y, 100);
    assert_eq!(normalized.width, 200);
    assert_eq!(normalized.height, 150);
}

#[test]
fn test_selection_is_valid() {
    // Valid selection
    let area = SelectionArea {
        x: 100,
        y: 100,
        width: 200,
        height: 150,
    };
    assert!(area.is_valid());

    // Zero width
    let area = SelectionArea {
        x: 100,
        y: 100,
        width: 0,
        height: 150,
    };
    assert!(!area.is_valid());

    // Zero height
    let area = SelectionArea {
        x: 100,
        y: 100,
        width: 200,
        height: 0,
    };
    assert!(!area.is_valid());

    // Negative (before normalization)
    let area = SelectionArea {
        x: 100,
        y: 100,
        width: -200,
        height: 150,
    };
    assert!(!area.is_valid());
}
