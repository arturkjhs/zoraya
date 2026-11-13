/*
 * libnsfb surface for a Zoraya window (TASK-27 9d, proposed ADR-0317).
 *
 * The buffer is the compositor's memory object, 32 bits a pixel; libnsfb
 * paints into it and `update` says what changed. The calls are
 * zoraya_window_* of libc.a (ports/netsurf/zoraya_window.h). Licence: MIT,
 * as libnsfb.
 */

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "libnsfb.h"
#include "libnsfb_plot.h"
#include "libnsfb_event.h"

#include "nsfb.h"
#include "surface.h"
#include "plot.h"

#include "zoraya_window.h"

#define UNUSED(x) ((void)(x))

#define MODIFIER_SHIFT 1u

/* Linux key numbers (the compositor's key codes) of the keys NetSurf uses that
 * have no code point, and of the main block's unshifted US characters: the
 * front end tracks the modifier keys itself and shifts the unshifted code. */
static int map_key(uint32_t code, uint32_t text)
{
	static const char main_row1[] = "\0\0" "1234567890-=";   /* codes 0..13 */
	static const char main_row2[] = "qwertyuiop[]";          /* codes 16..27 */
	static const char main_row3[] = "asdfghjkl;'`";          /* codes 30..41 */
	static const char main_row4[] = "\\zxcvbnm,./";          /* codes 43..53 */

	if (code >= 2 && code <= 13)
		return main_row1[code];
	if (code >= 16 && code <= 27)
		return main_row2[code - 16];
	if (code >= 30 && code <= 41)
		return main_row3[code - 30];
	if (code >= 43 && code <= 53)
		return main_row4[code - 43];
	switch (code) {
	case 1: return NSFB_KEY_ESCAPE;
	case 14: return NSFB_KEY_BACKSPACE;
	case 15: return NSFB_KEY_TAB;
	case 28: return NSFB_KEY_RETURN;
	case 29: return NSFB_KEY_LCTRL;
	case 42: return NSFB_KEY_LSHIFT;
	case 54: return NSFB_KEY_RSHIFT;
	case 56: return NSFB_KEY_LALT;
	case 57: return NSFB_KEY_SPACE;
	case 97: return NSFB_KEY_RCTRL;
	case 100: return NSFB_KEY_RALT;
	case 102: return NSFB_KEY_HOME;
	case 103: return NSFB_KEY_UP;
	case 104: return NSFB_KEY_PAGEUP;
	case 105: return NSFB_KEY_LEFT;
	case 106: return NSFB_KEY_RIGHT;
	case 107: return NSFB_KEY_END;
	case 108: return NSFB_KEY_DOWN;
	case 109: return NSFB_KEY_PAGEDOWN;
	case 110: return NSFB_KEY_INSERT;
	case 111: return NSFB_KEY_DELETE;
	case 125: return NSFB_KEY_LSUPER;
	case 126: return NSFB_KEY_RSUPER;
	default: break;
	}
	/* A key of another layout that types an ASCII character. */
	if (text >= 32 && text < 127)
		return (int)text;
	return NSFB_KEY_UNKNOWN;
}

static int button_key(uint32_t bit)
{
	switch (bit) {
	case 1: return NSFB_KEY_MOUSE_1;
	case 4: return NSFB_KEY_MOUSE_2;
	case 2: return NSFB_KEY_MOUSE_3;
	default: return NSFB_KEY_UNKNOWN;
	}
}

/* Points the nsfb at the window's buffer. */
static int take_buffer(nsfb_t *nsfb)
{
	uint32_t *pixels, w, h;

	if (zoraya_window_buffer(&pixels, &w, &h) != 0)
		return -1;
	nsfb->width = (int)w;
	nsfb->height = (int)h;
	nsfb->ptr = (uint8_t *)pixels;
	nsfb->linelen = (int)(w * 4);
	return 0;
}

static int zoraya_defaults(nsfb_t *nsfb)
{
	nsfb->width = 800;
	nsfb->height = 600;
	nsfb->format = NSFB_FMT_XRGB8888;
	select_plotters(nsfb);
	return 0;
}

static int zoraya_initialise(nsfb_t *nsfb)
{
	if (zoraya_window_open("NetSurf", (uint32_t)nsfb->width, (uint32_t)nsfb->height) != 0)
		return -1;
	if (take_buffer(nsfb) != 0) {
		zoraya_window_close();
		return -1;
	}
	return 0;
}

static int zoraya_finalise(nsfb_t *nsfb)
{
	nsfb->ptr = NULL;
	zoraya_window_close();
	return 0;
}

/* The compositor decides the size; a request for another size is kept as
 * the size to open with, and the window's own size wins once open. */
static int zoraya_set_geometry(nsfb_t *nsfb, int width, int height, enum nsfb_format_e format)
{
	if (nsfb->ptr == NULL) {
		if (width > 0)
			nsfb->width = width;
		if (height > 0)
			nsfb->height = height;
	}
	if (format != NSFB_FMT_ANY && format != NSFB_FMT_XRGB8888 && format != NSFB_FMT_ARGB8888)
		return -1;
	select_plotters(nsfb);
	return 0;
}

static int zoraya_update(nsfb_t *nsfb, nsfb_bbox_t *box)
{
	UNUSED(nsfb);
	zoraya_window_damage(box->x0, box->y0, box->x1 - box->x0, box->y1 - box->y0);
	return 0;
}

static bool zoraya_input(nsfb_t *nsfb, nsfb_event_t *event, int timeout)
{
	struct zoraya_window_event e;
	int waited = 0;

	for (;;) {
		int got = zoraya_window_poll(&e, timeout < 0);
		if (got < 0)
			return false;
		if (got == 0) {
			if (timeout == 0)
				return false;
			if (timeout > 0 && waited >= timeout) {
				event->type = NSFB_EVENT_CONTROL;
				event->value.controlcode = NSFB_CONTROL_TIMEOUT;
				return true;
			}
			/* A wait with a limit is a poll with a short sleep
			 * until the window library can wait on the channel
			 * with a deadline. */
			struct timespec ts = { 0, 2000000 };
			nanosleep(&ts, NULL);
			waited += 2;
			continue;
		}
		switch (e.kind) {
		case ZW_KEY: {
			int k = map_key(e.a, e.b);
			if (k == NSFB_KEY_UNKNOWN)
				continue;
			event->type = e.c ? NSFB_EVENT_KEY_DOWN : NSFB_EVENT_KEY_UP;
			event->value.keycode = (enum nsfb_key_code_e)k;
			return true;
		}
		case ZW_MOVE:
			event->type = NSFB_EVENT_MOVE_ABSOLUTE;
			event->value.vector.x = (int)e.a;
			event->value.vector.y = (int)e.b;
			event->value.vector.z = 0;
			return true;
		case ZW_BUTTON: {
			int k = button_key(e.c);
			if (k == NSFB_KEY_UNKNOWN)
				continue;
			event->type = e.d ? NSFB_EVENT_KEY_DOWN : NSFB_EVENT_KEY_UP;
			event->value.keycode = (enum nsfb_key_code_e)k;
			return true;
		}
		case ZW_RESIZE:
			if (take_buffer(nsfb) != 0)
				return false;
			event->type = NSFB_EVENT_RESIZE;
			event->value.resize.w = (int)e.a;
			event->value.resize.h = (int)e.b;
			return true;
		case ZW_CLOSE:
		case ZW_GONE:
			event->type = NSFB_EVENT_CONTROL;
			event->value.controlcode = NSFB_CONTROL_QUIT;
			return true;
		default:
			continue; /* focus: nothing for the front end */
		}
	}
}

const nsfb_surface_rtns_t zoraya_rtns = {
	.defaults = zoraya_defaults,
	.initialise = zoraya_initialise,
	.finalise = zoraya_finalise,
	.input = zoraya_input,
	.update = zoraya_update,
	.geometry = zoraya_set_geometry,
};

NSFB_SURFACE_DEF(zoraya, NSFB_SURFACE_ZORAYA, &zoraya_rtns)
