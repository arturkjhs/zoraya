/*
 * ncurses in a Zoraya window (ADR-0334, ADR-0383 Option C, ADR-0391 item 11):
 * a static C program built by zcc against libc.a and the sysroot's
 * libncursesw.a and libtinfow.a, run from zosh. One binary with named modes.
 *
 *   t_curses setup   setupterm of "xterm-256color" succeeds (the entry of
 *                    /usr/share/terminfo, from any current directory) and of
 *                    "no-such-terminal" gives the not-found result; the
 *                    entry's kbs is DEL, as the terminal sends (INV-0334-2,
 *                    INV-0391-2). Prints "t_curses: setup passed".
 *   t_curses keys    a curses program on the alternate screen: it prints the
 *                    window's size at start and again after each KEY_RESIZE,
 *                    and names each key it reads with keypad(TRUE): Backspace
 *                    is KEY_BACKSPACE, F1 is KEY_F(1) (INV-0334-3,
 *                    INV-0391-4). q ends it. Rows are never fixed by a test
 *                    (a big display has fewer than 24), the program prints
 *                    them; the columns are what a script reads.
 *   t_curses devtty [raw]  the first key from /dev/tty while fd 0 is a pipe
 *                    (what `/bin/cat FILE | less` does for its prompts)
 *   t_curses poll2   poll on a pipe nobody writes and on /dev/tty together
 *                    (less's check_poll): nothing ready, then a key wakes
 *                    the terminal only
 */
#define _GNU_SOURCE
#include <curses.h>
#include <fcntl.h>
#include <poll.h>
#include <string.h>
#include <term.h>
#include <termios.h>
#include <unistd.h>

static int bad;

#define CHECK(cond)                                                          \
	do {                                                                 \
		if (!(cond)) {                                               \
			printf("t_curses: %s:%d: %s FAIL\n", __func__, __LINE__, #cond); \
			bad++;                                               \
		}                                                            \
	} while (0)

static int setup(void)
{
	int err = 99;
	int r = setupterm("xterm-256color", 1, &err);
	CHECK(r == OK && err == 1);
	/* The entry says what the terminal sends for Backspace (ADR-0383 Option C). */
	char *kbs = tigetstr("kbs");
	CHECK(kbs != NULL && kbs != (char *)-1 && strcmp(kbs, "\177") == 0);
	char *kcub1 = tigetstr("kcub1");
	CHECK(kcub1 != NULL && kcub1 != (char *)-1 && strcmp(kcub1, "\033OD") == 0);
	CHECK(tigetnum("colors") == 256);
	del_curterm(cur_term);

	err = 99;
	r = setupterm("no-such-terminal", 1, &err);
	CHECK(r == ERR && err == 0);

	/* dumb has an entry too: the serial console's TERM. */
	err = 99;
	r = setupterm("dumb", 1, &err);
	CHECK(r == OK && err == 1);
	CHECK(tigetnum("cols") == 80);
	del_curterm(cur_term);

	if (bad) {
		printf("t_curses: setup FAIL\n");
		return 1;
	}
	printf("t_curses: setup passed\n");
	return 0;
}

static void size_line(const char *what)
{
	printw("%s cols %d rows %d\n", what, COLS, LINES);
	refresh();
}

static int keys(void)
{
	if (!initscr()) {
		printf("t_curses: initscr FAIL\n");
		return 1;
	}
	raw();
	noecho();
	keypad(stdscr, TRUE);
	scrollok(stdscr, TRUE);
	size_line("start");
	for (;;) {
		int c = getch();
		if (c == 'q')
			break;
		switch (c) {
		case ERR:
			printw("key ERR\n");
			break;
		case KEY_RESIZE:
			size_line("resize");
			continue;
		case KEY_BACKSPACE:
			printw("key KEY_BACKSPACE\n");
			break;
		case KEY_F(1):
			printw("key KEY_F(1)\n");
			break;
		case KEY_F(5):
			printw("key KEY_F(5)\n");
			break;
		case KEY_UP:
			printw("key KEY_UP\n");
			break;
		case KEY_DC:
			printw("key KEY_DC\n");
			break;
		case KEY_PPAGE:
			printw("key KEY_PPAGE\n");
			break;
		case 0x7f:
			printw("key DEL (not KEY_BACKSPACE)\n");
			break;
		default:
			printw("key 0x%x\n", c);
		}
		refresh();
	}
	endwin();
	printf("t_curses: keys done\n");
	return 0;
}

/* Keys from /dev/tty while fd 0 is a pipe: what `cat FILE | less` does for its prompts
 * (ADR-0331 item 3). Prints "t_curses: tty byte 0x.." for the first byte typed. */
static int devtty(int raw)
{
	int tty = open("/dev/tty", O_RDONLY);
	CHECK(tty >= 0);
	if (tty < 0)
		return 1;
	if (raw) {
		struct termios t;
		CHECK(tcgetattr(tty, &t) == 0);
		t.c_lflag &= ~(ICANON | ECHO);
		t.c_cc[VMIN] = 1;
		t.c_cc[VTIME] = 0;
		CHECK(tcsetattr(tty, TCSADRAIN, &t) == 0);
	}
	struct pollfd p = { tty, POLLIN, 0 };
	printf("t_curses: tty waiting\n");
	fflush(stdout);
	int r = poll(&p, 1, 20000);
	printf("t_curses: poll %d revents 0x%x\n", r, p.revents);
	CHECK(r == 1 && (p.revents & POLLIN));
	char c = 0;
	r = (int)read(tty, &c, 1);
	CHECK(r == 1);
	printf("t_curses: tty byte 0x%x\n", (unsigned char)c);
	return bad ? 1 : 0;
}

/* poll on fd 0 (a pipe nobody writes) and /dev/tty together, as less's check_poll does:
 * nothing is ready for 1 s, then a key wakes the terminal only. */
static int poll2(void)
{
	int tty = open("/dev/tty", O_RDONLY);
	CHECK(tty >= 0);
	if (tty < 0)
		return 1;
	struct pollfd p[2] = { { 0, POLLIN, 0 }, { tty, POLLIN, 0 } };
	int r = poll(p, 2, 1000);
	printf("t_curses: poll2 first %d revents %x %x\n", r, p[0].revents, p[1].revents);
	CHECK(r == 0);
	printf("t_curses: tty waiting\n");
	fflush(stdout);
	p[0].revents = p[1].revents = 0;
	r = poll(p, 2, 20000);
	printf("t_curses: poll2 second %d revents %x %x\n", r, p[0].revents, p[1].revents);
	CHECK(r == 1 && p[0].revents == 0 && (p[1].revents & POLLIN));
	return bad ? 1 : 0;
}

int main(int argc, char **argv)
{
	if (argc == 2 && strcmp(argv[1], "poll2") == 0)
		return poll2();
	if (argc >= 2 && strcmp(argv[1], "devtty") == 0)
		return devtty(argc == 3 && strcmp(argv[2], "raw") == 0);
	if (argc == 2 && strcmp(argv[1], "setup") == 0)
		return setup();
	if (argc == 2 && strcmp(argv[1], "keys") == 0)
		return keys();
	printf("usage: t_curses setup | keys\n");
	return 2;
}
