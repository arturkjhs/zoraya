/* Replaces nvi2's common/recover.c (TASK-31 ports-early, ADR-0335 item 5): no
 * recovery in stage 10. A crash loses the unsaved session; `vi -r` is not
 * offered; nothing is written to a recovery directory and nothing is mailed.
 * The buffer's record file is DB 1's own temporary file (dbopen with no name).
 * The entry points are the ones the rest of nvi calls (the PUBLIC lines of the
 * original), each doing nothing or saying so. */
#include <sys/types.h>
#include <sys/queue.h>

#include <bitstring.h>

#include "../common/common.h"

/*
 * rcv_tmp --
 *	Called when a file is opened: with no recovery file the buffer is anonymous.
 *
 * PUBLIC: int rcv_tmp(SCR *, EXF *, char *);
 */
int
rcv_tmp(SCR *sp, EXF *ep, char *name)
{
	(void)sp; (void)ep; (void)name;
	return (1);
}

/*
 * rcv_init --
 *	Called when a file is first modified: nothing to snapshot.
 *
 * PUBLIC: int rcv_init(SCR *);
 */
int
rcv_init(SCR *sp)
{
	(void)sp;
	return (0);
}

/*
 * rcv_sync --
 *	Called at each flush of the buffer's snapshot and on `:preserve`: nothing.
 *
 * PUBLIC: int rcv_sync(SCR *, u_int);
 */
int
rcv_sync(SCR *sp, u_int flags)
{
	(void)sp; (void)flags;
	return (0);
}

/*
 * rcv_list --
 *	`vi -r` with no file: there is nothing to list.
 *
 * PUBLIC: int rcv_list(SCR *);
 */
int
rcv_list(SCR *sp)
{
	msgq(sp, M_ERR, "Recovery is not available");
	return (1);
}

/*
 * rcv_read --
 *	`vi -r FILE`: there is nothing to read.
 *
 * PUBLIC: int rcv_read(SCR *, FREF *);
 */
int
rcv_read(SCR *sp, FREF *frp)
{
	(void)frp;
	msgq(sp, M_ERR, "Recovery is not available");
	return (1);
}
