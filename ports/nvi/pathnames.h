/* nvi2 2.2.2's pathnames.h for Zoraya (files/pathnames.h.in). The start-up
 * files are not read (ADR-0335 item 6: the patch removes the reads of the
 * system and the home files; only NEXINIT and EXINIT remain); the recovery
 * directory is a name nothing uses (recovery is removed, item 5). */
#include <paths.h>

#ifndef	_PATH_EXRC
#define	_PATH_EXRC	".exrc"
#endif
#ifndef	_PATH_MSGCAT
#define	_PATH_MSGCAT	"/usr/share/vi/catalog/"
#endif
#ifndef	_PATH_NEXRC
#define	_PATH_NEXRC	".nexrc"
#endif
#define	NVI_PATH_PRESERVE	"/tmp/vi.recover/"
#ifndef	_PATH_SYSEXRC
#define	_PATH_SYSEXRC	"/etc/vi.exrc"
#endif
#ifndef	_PATH_TAGS
#define	_PATH_TAGS	"tags"
#endif
