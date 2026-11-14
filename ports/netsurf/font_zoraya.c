/*
 * NetSurf's framebuffer font library over zoraya-text (ADR-0326, TASK-27
 * 9f-text), in place of font_internal.c's bitmap font (ADR-0318 item 5's
 * NETSURF_FB_FONTLIB, now `zoraya`). The calls are zoraya_text_* of libc.a
 * (zoraya_text.h): the system's fonts, read once from /system/fonts, which a
 * `window` line grants read only. The size of a glyph is the style's points
 * at NetSurf's dpi, which fb_font_init sets to 90 times the window's scale,
 * so that the page's lengths and its text are at the window's scale.
 * Licence: MIT; compiled into NetSurf's front end (GPL-2.0).
 */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include <libnsfb.h>
#include <libnsfb_plot.h>

#include "utils/errors.h"
#include "utils/log.h"
#include "netsurf/browser.h"
#include "netsurf/layout.h"
#include "netsurf/plot_style.h"
#include "netsurf/utf8.h"

#include "framebuffer/gui.h"
#include "framebuffer/font.h"
#include "framebuffer/zoraya_text.h"

static struct zoraya_text_style style_of(const plot_font_style_t *fstyle)
{
	struct zoraya_text_style s;

	s.family = (uint32_t)fstyle->family;
	s.weight = (int32_t)fstyle->weight;
	s.size = (int32_t)fstyle->size;
	s.dpi = (uint32_t)browser_get_dpi();
	return s;
}

bool fb_font_init(void)
{
	uint32_t scale = zoraya_window_scale();
	int n;

	/* NetSurf's 90 dpi at scale 1; browser_set_dpi keeps it in 72..250. */
	browser_set_dpi((int)(90 * (scale ? scale : 1)));
	n = zoraya_text_init();
	if (n < 4)
		NSLOG(netsurf, WARNING,
		      "%d of the 4 system fonts read from /system/fonts; the rest of the text is boxes", n);
	/* Never fatal: without fonts the text is measured and drawn as boxes. */
	return true;
}

bool fb_font_finalise(void)
{
	return true;
}

nserror fb_font_width(const plot_font_style_t *fstyle, const char *string, size_t length, int *width)
{
	struct zoraya_text_style s = style_of(fstyle);

	*width = zoraya_text_width(&s, string, length);
	return NSERROR_OK;
}

nserror fb_font_position(const plot_font_style_t *fstyle, const char *string, size_t length,
	int x, size_t *char_offset, int *actual_x)
{
	struct zoraya_text_style s = style_of(fstyle);

	*char_offset = zoraya_text_position(&s, string, length, x, actual_x);
	return NSERROR_OK;
}

static nserror fb_font_split(const plot_font_style_t *fstyle, const char *string, size_t length,
	int x, size_t *char_offset, int *actual_x)
{
	struct zoraya_text_style s = style_of(fstyle);

	*char_offset = zoraya_text_split(&s, string, length, x, actual_x);
	return NSERROR_OK;
}

struct plot_ctx {
	nsfb_t *nsfb;
	int x;
	int y;
	nsfb_colour_t colour;
};

static void plot_glyph(void *ctx, int32_t x, int32_t y, uint32_t width, uint32_t height, const uint8_t *coverage)
{
	struct plot_ctx *p = ctx;
	nsfb_bbox_t loc;

	loc.x0 = p->x + x;
	loc.y0 = p->y + y;
	loc.x1 = loc.x0 + (int)width;
	loc.y1 = loc.y0 + (int)height;
	nsfb_plot_glyph8(p->nsfb, &loc, coverage, (int)width, p->colour);
}

nserror fb_zoraya_plot_text(struct nsfb_s *nsfb, const struct plot_font_style *fstyle,
	int x, int y, const char *text, size_t length)
{
	struct zoraya_text_style s = style_of(fstyle);
	struct plot_ctx p;

	p.nsfb = nsfb;
	p.x = x;
	p.y = y;
	p.colour = fstyle->foreground;
	zoraya_text_glyphs(&s, text, length, plot_glyph, &p);
	return NSERROR_OK;
}

static struct gui_layout_table layout_table = {
	.width = fb_font_width,
	.position = fb_font_position,
	.split = fb_font_split,
};

struct gui_layout_table *framebuffer_layout_table = &layout_table;

/* UTF-8 is the local encoding, as with FreeType: no table. */
struct gui_utf8_table *framebuffer_utf8_table = NULL;
