/* nvi2 2.2.2's config.h for Zoraya (TASK-31 ports-early, ADR-0335 item 1): what
 * files/config.h.in's cmakedefines are for this port. Wide characters, no iconv
 * (ADR-0342 item 9), no pty (the `:script` command is removed, ADR-0338). */
#define USE_WIDECHAR 1
#define HAVE_NCURSES_H 1
#define HAVE_TERM_H 1
#define HAVE_STRUCT_STAT_ST_MTIM 1
