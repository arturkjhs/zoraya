/*
 * NetSurf's framebuffer font library over zoraya-text (ADR-0326), included by
 * framebuffer/font.h when FB_USE_ZORAYA is defined (ports/netsurf/Makefile.config,
 * patch 0008). Licence: MIT; compiled into NetSurf's front end (GPL-2.0).
 */
#ifndef NETSURF_FB_FONT_ZORAYA_H
#define NETSURF_FB_FONT_ZORAYA_H

#include <stddef.h>

struct nsfb_s;
struct plot_font_style;

/* Plots `length` bytes of UTF-8 with the pen's origin at (x, y), y the
 * baseline, through nsfb_plot_glyph8 (which clips). */
nserror fb_zoraya_plot_text(struct nsfb_s *nsfb, const struct plot_font_style *fstyle,
	int x, int y, const char *text, size_t length);

#endif
