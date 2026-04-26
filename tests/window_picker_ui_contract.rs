#[test]
fn window_picker_uses_reduced_toolbar_and_warm_hover_contract() {
    let source = include_str!("../capture-overlay/src/WindowPickerOverlay.cpp");

    assert!(
        source.contains("WINDOW_PICKER_TOOL_INDICES[] = {1, 3}")
            || source.contains("WINDOW_PICKER_TOOL_INDICES[] = { 1, 3 }")
            || source.contains("kWindowPickerToolIndices[] = {1, 3}"),
        "window picker should expose only Area and Window toolbar tools"
    );

    assert!(
        source.contains("for (int i = m_thumbnailRects.size() - 1; i >= 0; --i)")
            || source.contains(
                "for (int i = static_cast<int>(m_thumbnailRects.size()) - 1; i >= 0; --i)"
            ),
        "window picker hover hit testing should walk cards from topmost to bottommost"
    );

    assert!(
        source.contains("QColor(176, 92, 56") || source.contains("QColor(255, 212, 178"),
        "window picker should use the same warm accent family as the capture toolbar"
    );

    assert!(
        !source.contains("QColor(0, 122, 255") && !source.contains("QColor(90, 170, 255"),
        "window picker should not use the old blue hover accent anymore"
    );

    assert!(
        source.contains("drawWindowCard(p, thumb, win, scaled, false);")
            && source.contains("drawWindowCard(p, m_thumbnailRects[m_hoveredIdx], m_windows[m_hoveredIdx], scaled, true);"),
        "window picker should render base cards first and the hovered card in a dedicated second pass"
    );
}
