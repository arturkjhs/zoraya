/* The window calls of Zoraya's libc.a (proposed ADR-0317; crates/zoraya-posix,
 * src/ffi/window.rs). A program has a window only when its description says
 * `window`: otherwise zoraya_window_open returns -1. */
#ifndef ZORAYA_WINDOW_H
#define ZORAYA_WINDOW_H

#include <stdint.h>

enum zoraya_window_event_kind {
	ZW_KEY = 1,    /* a: key code (Linux's), b: code point or 0, c: 1 press, d: modifiers */
	ZW_MOVE = 2,   /* a, b: the pixel */
	ZW_BUTTON = 3, /* a, b: the pixel, c: button bit (1 left, 2 right, 4 middle), d: 1 press */
	ZW_FOCUS = 4,  /* a: 1 gained */
	ZW_RESIZE = 5, /* a, b: the new buffer size; read the buffer again */
	ZW_CLOSE = 6,  /* the person asked to close the window */
	ZW_GONE = 7,   /* the compositor is gone */
	ZW_WHEEL = 8   /* a, b: the pixel, c, d: the horizontal and vertical turn as
	                * int32_t in 120ths of a notch, d > 0 toward the person
	                * (scroll down), c > 0 to the right (proposed ADR-0322) */
};

struct zoraya_window_event {
	uint32_t kind, a, b, c, d;
};

int zoraya_window_open(const char *title, uint32_t width, uint32_t height);
int zoraya_window_buffer(uint32_t **pixels, uint32_t *width, uint32_t *height);
void zoraya_window_damage(int x, int y, int width, int height);
int zoraya_window_poll(struct zoraya_window_event *event, int block);
void zoraya_window_close(void);

#endif
