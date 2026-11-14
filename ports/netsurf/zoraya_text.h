/*
 * The text calls of libc.a (ADR-0326): zoraya-text over the system's fonts,
 * which are read once, by zoraya_text_init, from /system/fonts and nowhere
 * else (crates/zoraya-posix/src/ffi/text.rs, crates/zoraya-fontset). One
 * thread at a time; a call made while another is inside gets 0 (-1 from
 * zoraya_text_init and zoraya_text_metrics). Licence: MIT, as libnsfb.
 */
#ifndef ZORAYA_TEXT_H
#define ZORAYA_TEXT_H

#include <stddef.h>
#include <stdint.h>

/* A style: NetSurf's generic family (plot_font_generic_family_t) and CSS
 * weight, the size in points as 22.10 fixed point (plot_style_fixed), and the
 * dpi the size is drawn at. */
struct zoraya_text_style {
	uint32_t family;
	int32_t weight;
	int32_t size;
	uint32_t dpi;
};

/* How many of the four system fonts were read and parsed (0 to 4). */
int zoraya_text_init(void);
/* The advance of `len` bytes of UTF-8, in pixels. */
int zoraya_text_width(const struct zoraya_text_style *style, const char *text, size_t len);
/* The character boundary nearest to x: its offset, its x in *actual_x. */
size_t zoraya_text_position(const struct zoraya_text_style *style, const char *text, size_t len, int x, int *actual_x);
/* Where to break the text to fit x (NetSurf's split): its offset, its x. */
size_t zoraya_text_split(const struct zoraya_text_style *style, const char *text, size_t len, int x, int *actual_x);
/* The face's ascent and descent in pixels; 0, or -1 before init. */
int zoraya_text_metrics(const struct zoraya_text_style *style, int *ascent, int *descent);
/* Calls plot for every glyph: its top left from the pen's origin on the
 * baseline, its size, and width x height bytes of coverage; gives the
 * advance. plot must not call zoraya_text_*. */
int zoraya_text_glyphs(const struct zoraya_text_style *style, const char *text, size_t len,
	void (*plot)(void *ctx, int32_t x, int32_t y, uint32_t width, uint32_t height, const uint8_t *coverage),
	void *ctx);
/* The window's scale (1 with no window), ADR-0164. */
uint32_t zoraya_window_scale(void);

#endif
