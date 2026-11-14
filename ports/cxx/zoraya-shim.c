/* The names libc++ references that libc.a does not export (TASK-30, proposed
 * ADR-0366), linked into libc++.a: std::filesystem's and std::this_thread's
 * calls that libposix does not serve under the POSIX name. Each either does
 * what the call says through calls libposix has (truncate, sched_yield,
 * pathconf's two limits) or fails with an error the caller reports (link and
 * statvfs: std::filesystem::create_hard_link and space throw a
 * filesystem_error); none reports a success that did not happen. They are
 * weak, so that a strong definition already linked wins without a clash; a
 * linker does not pull an archive member to replace a weak definition, so
 * the day libposix exports one of these names, it is removed here (ADR-0366's
 * revisit trigger). */
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <sys/statvfs.h>
#include <unistd.h>

#define WEAK __attribute__((weak))

/* libposix's sched_yield under its own name (ADR-0308 item 4), as
 * ports/apps/pyshim.c uses it: std::this_thread::yield and libc++'s
 * spin-then-sleep waits. */
long zoraya_posix_sched_yield(void);

WEAK int sched_yield(void)
{
	return (int)zoraya_posix_sched_yield();
}

/* std::filesystem::resize_file: libposix has ftruncate. */
WEAK int truncate(const char *path, off_t length)
{
	int fd = open(path, O_WRONLY | O_CLOEXEC);
	if (fd < 0)
		return -1;
	int r = ftruncate(fd, length);
	int saved = errno;
	close(fd);
	errno = saved;
	return r;
}

/* std::filesystem::create_hard_link: hard links are never created
 * (ADR-0013). */
WEAK int link(const char *existing, const char *new_path)
{
	(void)existing;
	(void)new_path;
	errno = ENOTSUP;
	return -1;
}

/* std::filesystem::space: libposix has no call that reports a volume's free
 * space. */
WEAK int statvfs(const char *restrict path, struct statvfs *restrict buf)
{
	(void)path;
	(void)buf;
	errno = ENOTSUP;
	return -1;
}

/* std::filesystem::current_path sizes its getcwd buffer with
 * pathconf(".", _PC_PATH_MAX): the limits of <limits.h>, the ones every C
 * program here is built with. */
WEAK long pathconf(const char *path, int name)
{
	(void)path;
	switch (name) {
	case _PC_PATH_MAX:
		return PATH_MAX;
	case _PC_NAME_MAX:
		return NAME_MAX;
	default:
		errno = EINVAL;
		return -1;
	}
}
