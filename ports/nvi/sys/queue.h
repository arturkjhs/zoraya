/* <sys/queue.h> for nvi2 (TASK-31 ports-early): the singly-linked list and
 * tail queue macros nvi2 uses, with the semantics of the BSD header of the
 * same name, which musl does not ship. Written for this repository (small
 * enough not to be a dependency), under the repository's licence. Only what
 * nvi2 2.2.2 uses is here. */
#ifndef ZORAYA_SYS_QUEUE_H
#define ZORAYA_SYS_QUEUE_H

#include <stddef.h>

/* Singly-linked list. */
#define SLIST_HEAD(name, type)	struct name { struct type *slh_first; }
#define SLIST_ENTRY(type)	struct { struct type *sle_next; }
#define SLIST_EMPTY(head)	((head)->slh_first == NULL)
#define SLIST_FIRST(head)	((head)->slh_first)
#define SLIST_NEXT(elm, field)	((elm)->field.sle_next)
#define SLIST_INIT(head)	do { (head)->slh_first = NULL; } while (0)
#define SLIST_FOREACH(var, head, field) \
	for ((var) = SLIST_FIRST(head); (var); (var) = SLIST_NEXT(var, field))
#define SLIST_FOREACH_SAFE(var, head, field, tvar) \
	for ((var) = SLIST_FIRST(head); (var) && ((tvar) = SLIST_NEXT(var, field), 1); (var) = (tvar))
#define SLIST_INSERT_HEAD(head, elm, field) do { \
	SLIST_NEXT(elm, field) = SLIST_FIRST(head); \
	SLIST_FIRST(head) = (elm); \
} while (0)
#define SLIST_INSERT_AFTER(slistelm, elm, field) do { \
	SLIST_NEXT(elm, field) = SLIST_NEXT(slistelm, field); \
	SLIST_NEXT(slistelm, field) = (elm); \
} while (0)
#define SLIST_REMOVE_HEAD(head, field) do { \
	SLIST_FIRST(head) = SLIST_NEXT(SLIST_FIRST(head), field); \
} while (0)
#define SLIST_REMOVE_AFTER(elm, field) do { \
	SLIST_NEXT(elm, field) = SLIST_NEXT(SLIST_NEXT(elm, field), field); \
} while (0)

/* Tail queue. */
#define TAILQ_HEAD(name, type)	struct name { struct type *tqh_first; struct type **tqh_last; }
#define TAILQ_ENTRY(type)	struct { struct type *tqe_next; struct type **tqe_prev; }
#define TAILQ_EMPTY(head)	((head)->tqh_first == NULL)
#define TAILQ_FIRST(head)	((head)->tqh_first)
#define TAILQ_NEXT(elm, field)	((elm)->field.tqe_next)
#define TAILQ_LAST(head, headname) \
	(*(((struct headname *)((head)->tqh_last))->tqh_last))
#define TAILQ_PREV(elm, headname, field) \
	(*(((struct headname *)((elm)->field.tqe_prev))->tqh_last))
#define TAILQ_INIT(head) do { \
	(head)->tqh_first = NULL; \
	(head)->tqh_last = &(head)->tqh_first; \
} while (0)
#define TAILQ_FOREACH(var, head, field) \
	for ((var) = TAILQ_FIRST(head); (var); (var) = TAILQ_NEXT(var, field))
#define TAILQ_FOREACH_SAFE(var, head, field, tvar) \
	for ((var) = TAILQ_FIRST(head); (var) && ((tvar) = TAILQ_NEXT(var, field), 1); (var) = (tvar))
#define TAILQ_FOREACH_REVERSE(var, head, headname, field) \
	for ((var) = TAILQ_LAST(head, headname); (var); (var) = TAILQ_PREV(var, headname, field))
#define TAILQ_INSERT_HEAD(head, elm, field) do { \
	if ((TAILQ_NEXT(elm, field) = TAILQ_FIRST(head)) != NULL) \
		TAILQ_FIRST(head)->field.tqe_prev = &TAILQ_NEXT(elm, field); \
	else \
		(head)->tqh_last = &TAILQ_NEXT(elm, field); \
	TAILQ_FIRST(head) = (elm); \
	(elm)->field.tqe_prev = &TAILQ_FIRST(head); \
} while (0)
#define TAILQ_INSERT_TAIL(head, elm, field) do { \
	TAILQ_NEXT(elm, field) = NULL; \
	(elm)->field.tqe_prev = (head)->tqh_last; \
	*(head)->tqh_last = (elm); \
	(head)->tqh_last = &TAILQ_NEXT(elm, field); \
} while (0)
#define TAILQ_INSERT_AFTER(head, listelm, elm, field) do { \
	if ((TAILQ_NEXT(elm, field) = TAILQ_NEXT(listelm, field)) != NULL) \
		TAILQ_NEXT(elm, field)->field.tqe_prev = &TAILQ_NEXT(elm, field); \
	else \
		(head)->tqh_last = &TAILQ_NEXT(elm, field); \
	TAILQ_NEXT(listelm, field) = (elm); \
	(elm)->field.tqe_prev = &TAILQ_NEXT(listelm, field); \
} while (0)
#define TAILQ_INSERT_BEFORE(listelm, elm, field) do { \
	(elm)->field.tqe_prev = (listelm)->field.tqe_prev; \
	TAILQ_NEXT(elm, field) = (listelm); \
	*(listelm)->field.tqe_prev = (elm); \
	(listelm)->field.tqe_prev = &TAILQ_NEXT(elm, field); \
} while (0)
#define TAILQ_REMOVE(head, elm, field) do { \
	if (TAILQ_NEXT(elm, field) != NULL) \
		TAILQ_NEXT(elm, field)->field.tqe_prev = (elm)->field.tqe_prev; \
	else \
		(head)->tqh_last = (elm)->field.tqe_prev; \
	*(elm)->field.tqe_prev = TAILQ_NEXT(elm, field); \
} while (0)

#endif
