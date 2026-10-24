# W8 (TASK-02b): synthetic recursive build. Level 1: this file. Level 2: a/ and b/. Leaves: a/1 a/2 b/1 b/2.
# Every leaf has 8 independent table-generator targets, 8 objects and an archive (leaf.mk), so -jN keeps all
# job slots busy across four make processes that share one jobserver.
SUBDIRS = a b

all: $(SUBDIRS)

$(SUBDIRS):
	$(MAKE) -C $@

clean:
	rm -f */*/t?.h */*/m?.o */*/mktab */*/libleaf.a

.PHONY: all clean $(SUBDIRS)
