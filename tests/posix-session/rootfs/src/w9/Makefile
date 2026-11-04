# W9 (TASK-02b) as a scenario: a recursive parallel build of three levels,
# each leaf writing a partial target, to be interrupted.
SUBDIRS = a b
all: $(SUBDIRS)
$(SUBDIRS):
	$(MAKE) -C $@
.PHONY: all $(SUBDIRS)
