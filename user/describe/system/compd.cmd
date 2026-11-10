name     compd
summary  the compositor: windows, their frames, the dock and the bar
usage    compd
cwd      none
setting  gui.theme             choice light|dark  light  light or dark, for the frames and every window
setting  gui.motion            yes-no yes  windows and the switcher move, rather than appear
setting  gui.scale             choice auto|1|2  auto  screen pixels per point; auto is 2 on a display 1920 wide or wider, and follows the display when its size changes
setting  gui.wallpaper         text   ''  the picture behind the windows; '' is the theme's own
setting  gui.keyboard-layouts  list of choice us|ua|ru|cz 1 4 unique 'us'  the keyboard layouts System+Space switches between, in that order
setting  gui.keyboard-layout   choice us|ua|ru|cz  us  the layout in use; one of gui.keyboard-layouts
---
The compositor draws every window's frame, the bar and the dock, and is the
only reader of the keyboard and the pointer (ADR-0093, ADR-0160). It reads
the gui settings in /config/settings when it starts; the settings window
applies a change at once, the settings command at the next session.

FILES
  /config/settings   settings

SEE ALSO
  help settings
