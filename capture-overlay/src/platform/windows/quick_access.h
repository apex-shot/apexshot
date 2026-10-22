#pragma once

class QApplication;

/// Quick Access shell for the Windows build.
///
/// A small always-on-top bar that offers the three capture modes and shows a
/// result card after each one. This is the first visible surface of the port;
/// it deliberately reuses Platform:: rather than the Linux overlay paths.
namespace QuickAccess {

/// Creates the bar, shows it, and runs the event loop to completion.
int run(QApplication& app);

} // namespace QuickAccess
