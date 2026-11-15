#!/usr/bin/env ruby
# Which steps a change of the Makefile needs (ADR-0348, item 2 of the architect's
# decision): not every step, but the steps whose recipes or variables the diff
# changes.
#
#   tools/ci-makefile-steps.rb OLD_REV "STEP STEP ..."   (OLD_REV: a git revision of the Makefile;
#                                                        the new one is ./Makefile)
#   tools/ci-makefile-steps.rb --files OLD NEW "STEP ..."  (two files, for tests)
#
# Prints one line on stdout: `full` (the change cannot be attributed to a rule
# or a variable: .PHONY, an include, an export, a conditional, a line that is
# not an assignment or a rule), or `steps: A B C` (the STEPS the change reaches,
# maybe none), and `unit=1` on a line of its own if it reaches the recipe of
# `test-unit`. The reasoning goes to stderr. Any failure exits non-zero: the
# caller then takes the map's line for Makefile (every step).
#
# How: each version of the Makefile is parsed into rules (a header line with
# its recipe lines) and variables (an assignment or a define block, with
# continuation lines). `diff -U0` gives the changed lines of both versions;
# each belongs to a rule (a recipe line, a header line) or a variable, to
# nothing (a blank or comment line) or to nothing one can attribute (full). A
# step reaches a rule or a variable if it is in the closure of the step's rules:
# prerequisites (variables expanded, `$(addprefix P,...)` and a word with an
# unexpandable part standing for every target that begins with the text before
# it), `$(MAKE) TARGET` in a recipe, and variables referenced in a prerequisite
# list, a recipe or another variable's value ($(VAR), ${VAR}, $(call VAR,...)).
# A change of a variable in the closure of `ci-build` and of no step runs the
# boot step, so that the build job runs. The old version answers for the lines
# that were removed, the new one for those that were added.
#
# "Reaches no step" is not "affects nothing": a changed rule or variable that no
# step, `test-unit`, `ci-build` or listed non-step root (ROOTS: check-*, run*,
# mutants*, ...) reaches is `full`, so every blind spot of the parser falls on the
# safe side. Also `full`: a change of SHELL, MAKEFLAGS and the like, of an
# exported variable, of a variable a `$(eval` template uses, any line that begins
# with `export`/`unexport`, and a change that moves an unchanged line to another
# owner (a deleted rule header, a removed trailing backslash). A recipe or
# prerequisite word the tool cannot resolve (`$(MAKE) $(2)`, a `$(foreach ...)`,
# a computed variable name) stands for every rule (every variable).
require 'open3'
require 'tempfile'

def die(msg)
  warn "ci-makefile-steps: #{msg}"
  exit 2
end

class Mk
  attr_reader :owner, :vars, :rules, :eval_refs, :exported

  CONDITIONAL = /\A\s*(ifeq|ifneq|ifdef|ifndef|else|endif)\b/
  ASSIGN = /\A\s*(?:(?:export|override|private)\s+)*([A-Za-z_][\w.\-]*)\s*(::=|:=|\?=|\+=|!=|=)(.*)\z/m

  def initialize(text)
    lines = text.lines.map(&:chomp)
    @owner = Array.new(lines.size, :ignore)
    @vars = Hash.new { |h, k| h[k] = [] }
    @rules = [] # { targets: [raw words], raw: String, prereq: String, text: String }
    @eval_refs = []
    @exported = []
    cur = nil   # the rule whose recipe lines come next
    define = nil
    i = 0
    while i < lines.size
      j = i
      j += 1 while j + 1 < lines.size && lines[j] =~ /(?<!\\)(?:\\\\)*\\\z/
      stmt = (i..j).map { |k| lines[k] }
      first = stmt[0]
      mark = ->(o) { (i..j).each { |k| @owner[k] = o } }
      if define
        mark.call([:v, define])
        if first =~ /\A\s*endef\b/
          define = nil
        else
          @vars[define] << stmt.join(' ')
        end
      elsif first =~ /\A\s*define\s+([A-Za-z_][\w.\-]*)/
        define = Regexp.last_match(1)
        @vars[define] # exists
        mark.call([:v, define])
        cur = nil
      elsif first.strip.empty? || first =~ /\A\s*#/ && first !~ /\A\t/
        mark.call(:ignore)
      elsif first.start_with?("\t")
        if first.strip.start_with?('#') && i == j
          mark.call(:ignore)
        elsif cur
          @rules[cur][:text] << ' ' << stmt.join(' ')
          mark.call([:r, cur])
        else
          mark.call(:unknown)
        end
      elsif first =~ CONDITIONAL
        mark.call(:unknown)
      elsif first =~ /\A\s*(?:export|unexport)\b/
        mark.call(:unknown)
        stmt.join(' ').sub(/\A\s*(?:export|unexport)\s*/, '').split(/(?:::=|:=|\?=|\+=|=)/).first.to_s.split.each { |w| @exported << w }
        cur = nil
      elsif stmt.join(' ').include?('$(eval') || stmt.join(' ').include?('${eval')
        mark.call(:unknown)
        (@eval_refs ||= []).concat(refs(stmt.join(' ')))
        cur = nil
      elsif (m = ASSIGN.match(stmt.join(' '))) && m[1] != '' && first !~ /\A\s*(include|-include|sinclude)\b/
        @vars[m[1]] << m[3]
        mark.call([:v, m[1]])
        cur = nil
      elsif first =~ /\A\s*(-?include|sinclude|export|unexport|vpath)\b/ || first =~ /\A\s*\.[A-Z_]+\s*:/
        mark.call(:unknown)
        cur = nil
      elsif (hdr = header(stmt.join(' ')))
        @rules << hdr
        cur = @rules.size - 1
        mark.call([:r, cur])
      else
        mark.call(:unknown)
        cur = nil
      end
      i = j + 1
    end
  end

  # [targets, rest] split at the first top-level colon, or nil.
  def header(s)
    depth = 0
    s.each_char.with_index do |c, k|
      depth += 1 if c == '(' || c == '{'
      depth -= 1 if c == ')' || c == '}'
      next unless c == ':' && depth.zero?
      return nil if s[k + 1] == '=' # an assignment the pattern missed
      targets = s[0...k].strip
      rest = s[(k + 1)..].sub(/\A:/, '')
      return nil if targets.empty? || targets =~ /\A\s*\t/
      prereq, _, recipe = rest.partition(';')
      return { targets: words(expand(targets)), raw: targets, prereq: prereq, text: "#{prereq} #{recipe}".dup }
    end
    nil
  end

  # Replaces $(NAME) and ${NAME} by the value of a variable that has one.
  def expand(str, depth = 0)
    return str if depth > 8
    str.gsub(/\$\(([A-Za-z_][\w.\-]*)\)|\$\{([A-Za-z_][\w.\-]*)\}/) do
      name = Regexp.last_match(1) || Regexp.last_match(2)
      v = @vars.key?(name) ? @vars[name] : nil
      v.nil? || v.empty? ? Regexp.last_match(0) : expand(v.join(' '), depth + 1)
    end
  end

  # Words of a string, a $(...) call staying one word.
  def words(str)
    out = []
    cur = +''
    depth = 0
    str.each_char do |c|
      if c =~ /\s/ && depth.zero?
        out << cur unless cur.empty?
        cur = +''
        next
      end
      depth += 1 if c == '(' || c == '{'
      depth -= 1 if c == ')' || c == '}'
      cur << c
    end
    out << cur unless cur.empty?
    out.flat_map { |w| addprefix(w) }
  end

  # $(addprefix P,LIST): each word of LIST with P before it; a word that
  # cannot be expanded is P and the unexpanded rest, which matches by prefix.
  def addprefix(word)
    m = /\A\$\(addprefix\s+([^,]*),(.*)\)\z/m.match(word)
    return [word] unless m
    pre = m[1]
    words(m[2]).map { |w| w.include?('$(') ? "#{pre}$(?)" : "#{pre}#{w}" }
  end

  # Names of the variables a text references; '*' stands for every variable (a computed name).
  def refs(text)
    t = text.gsub('$$', '')
    r = t.scan(/\$[({]([A-Za-z_][\w.\-]*)(?::[^)}]*)?[)}]/).flatten
    r += t.scan(/\$[({](?:call|origin|value|flavor)\s+([A-Za-z_][\w.\-]*)/).flatten
    r << '*' if t =~ /\$[({]\$[({]/ || t =~ /\$[({][\w.\-]+\$[({]/
    r.uniq
  end

  # Targets a recipe runs through make: $(MAKE) [flags] T..., ${MAKE}, make; a word that
  # cannot be resolved ($(2), $(foreach ...), $$X) stands for every rule ('$(?)').
  def makes(text)
    out = []
    text.scan(/(?:\$[({]MAKE[)}]|(?<![\w$-])make(?![\w-]))((?:\s+[^;&|\n]*)?)/) do |(args)|
      toks = words(expand(args))
      skip = false
      toks.each do |w|
        if skip then skip = false; next end
        if w == '-f' || w == '-C' || w == '--file' || w == '--directory' then skip = true; next end
        next if w.start_with?('-')
        next if w.include?('=') && !w.include?('$')
        w = w.gsub(/\A["']|["']\z/, '')
        next if w.empty? || w == '\\'
        if w.include?('$$')
          pre = w.split('$$').first.to_s
          out << "#{pre}$(?)"
        elsif w.include?('$(') || w.include?('${')
          out << "#{w[0...w.index(/\$[({]/)]}$(?)"
        else
          out << w
        end
      end
    end
    out
  end

  def rules_for(word)
    if word.include?('$(')
      pre = word[0...word.index('$(')]
      return @rules.each_index.to_a if pre.empty?
      return @rules.each_index.select { |i| @rules[i][:targets].any? { |t| t.start_with?(pre) } }
    end
    @rules.each_index.select do |i|
      @rules[i][:targets].any? do |t|
        t == word || (t.include?('%') && Regexp.new("\\A#{Regexp.escape(t).sub('%', '.+')}\\z").match?(word))
      end
    end
  end

  def rules_for_pattern(re)
    @rules.each_index.select { |i| @rules[i][:targets].any? { |t| re.match?(t) } }.map { |i| [:r, i] }
  end

  # What owns line n (1-based) independently of the rules' numbers.
  def key(n)
    o = @owner[n - 1]
    case o
    when Array then o[0] == :r ? [:r, @rules[o[1]][:targets]] : o
    else o
    end
  end

  def var_nodes(v)
    v == '*' ? @vars.keys.map { |k| [:v, k] } : [[:v, v]]
  end

  # The nodes reached from the rules of a target: [:r, i] and [:v, name].
  def closure(target)
    seen = {}
    todo = target.is_a?(Regexp) ? rules_for_pattern(target) : rules_for(target).map { |i| [:r, i] }
    until todo.empty?
      n = todo.pop
      next if seen[n]
      seen[n] = true
      if n[0] == :r
        rule = @rules[n[1]]
        words(expand(rule[:prereq])).each { |w| todo.concat(rules_for(w).map { |i| [:r, i] }) }
        makes(rule[:text]).each { |w| todo.concat(rules_for(w).map { |i| [:r, i] }) }
        (refs(rule[:text]) + refs(rule[:prereq]) + refs(rule[:raw].to_s)).uniq.each { |v| todo.concat(var_nodes(v)) }
      else
        @vars[n[1]].each { |val| refs(val).each { |v| todo.concat(var_nodes(v)) } }
      end
    end
    seen
  end
end

def hunks(old, new)
  out, st = Open3.capture2('diff', '-U0', old, new)
  die('diff failed') unless st.exitstatus <= 1
  hs = []
  out.each_line do |l|
    next unless (m = /\A@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.match(l))
    hs << [m[1].to_i, (m[2] || '1').to_i, m[3].to_i, (m[4] || '1').to_i]
  end
  hs
end

# Pairs of unchanged lines [old line, new line].
def unchanged_pairs(hs, nold, nnew)
  pairs = []
  oi = 1
  ni = 1
  hs.each do |a, b, c, d|
    ofin = b.zero? ? a : a - 1
    nfin = d.zero? ? c : c - 1
    (oi..ofin).zip(ni..nfin).each { |o, n| pairs << [o, n] if o && n }
    oi = b.zero? ? a + 1 : a + b
    ni = d.zero? ? c + 1 : c + d
  end
  (oi..nold).zip(ni..nnew).each { |o, n| pairs << [o, n] if o && n }
  pairs
end

SPECIAL = %w[SHELL .SHELLFLAGS MAKEFLAGS GNUMAKEFLAGS MAKEFILES VPATH MAKESHELL MAKE .RECIPEPREFIX].freeze
# Targets whose recipes and variables no test step runs: a change that only these reach affects no step.
ROOTS = /\A(check(-.*)?|fmt-check|clippy|adr-check|mutants(-host)?|run(-.*)?|debug|clean(-.*)?|measure-.*|bench-.*|hand-.*|print-.*|test|test-changed|test-par|build|all|ramdisk|posix-table|tour-scripts|test-posix-long|test-python-long|fetch-ports|ports)\z/

def main(old, new, steps)
  mo = Mk.new(File.read(old))
  mn = Mk.new(File.read(new))
  hs = hunks(old, new)
  olds = hs.flat_map { |a, b, _, _| (a...(a + b)).to_a }
  news = hs.flat_map { |_, _, c, d| (c...(c + d)).to_a }
  units = [] # [model, node]
  full = nil
  { mo => olds, mn => news }.each do |m, ls|
    ls.each do |n|
      o = m.owner[n - 1]
      case o
      when :ignore, nil then next
      when :unknown
        full ||= "line #{n} of #{m.equal?(mo) ? 'the old' : 'the new'} Makefile is not an assignment, a rule or a recipe line"
      else units << [m, o]
      end
    end
  end
  unchanged_pairs(hs, mo.owner.size, mn.owner.size).each do |o, n|
    next if mo.key(o) == mn.key(n)
    full ||= "line #{o} of the old Makefile (#{n} of the new) is not changed but belongs to something else now"
  end
  special = (SPECIAL + mo.exported + mn.exported).uniq
  evalv = lambda do |m|
    set = m.eval_refs.dup
    todo = set.dup
    until todo.empty?
      v = todo.pop
      m.vars[v].each { |val| m.refs(val).each { |r| (set << r; todo << r) unless set.include?(r) } }
    end
    set
  end
  units.each do |m, node|
    next unless node[0] == :v
    if special.include?(node[1])
      full ||= "variable #{node[1]} reaches every recipe (it is special or exported)"
    elsif evalv.call(m).include?(node[1]) || evalv.call(m).include?('*')
      full ||= "variable #{node[1]} is used by a $(eval template"
    end
  end
  if full
    warn "ci-makefile-steps: #{full}: every step"
    puts 'full'
    return
  end
  got = []
  unit = false
  build = false
  lost = []
  closures = Hash.new { |h, k| h[k] = {} }
  cl = ->(m, t) { closures[[m.object_id, t]] = m.closure(t) unless closures.key?([m.object_id, t]); closures[[m.object_id, t]] }
  units.uniq.each do |m, node|
    hit = false
    steps.each do |s|
      next unless cl.call(m, s).key?(node)
      got << s
      hit = true
    end
    if cl.call(m, 'test-unit').key?(node)
      unit = true
      hit = true
    end
    if cl.call(m, 'ci-build').key?(node)
      build = true
      hit = true
    end
    rooted = cl.call(m, ROOTS).key?(node)
    what = node[0] == :v ? "variable #{node[1]}" : "rule #{m.rules[node[1]][:targets].join(' ')}"
    warn "ci-makefile-steps: #{what}: #{hit ? 'reaches steps' : rooted ? 'reaches a root that is no step' : 'reaches nothing'}"
    lost << what unless hit || rooted
  end
  unless lost.empty?
    warn "ci-makefile-steps: #{lost.uniq.join(', ')}: reached by no step and no known root, so not known to be harmless: every step"
    puts 'full'
    return
  end
  got << 'test-boot' if build && got.empty?
  puts "steps:#{got.uniq.sort.map { |x| " #{x}" }.join}"
  puts 'unit=1' if unit
end

args = ARGV.dup
if args[0] == '--files'
  _, old, new, steps = args
else
  rev, steps = args
  die('usage: ci-makefile-steps.rb OLD_REV "STEPS" | --files OLD NEW "STEPS"') unless rev && steps
  tf = Tempfile.new('mk-old')
  text, st = Open3.capture2('git', 'show', "#{rev}:Makefile")
  die("git show #{rev}:Makefile failed") unless st.success?
  tf.write(text)
  tf.flush
  old = tf.path
  new = 'Makefile'
end
main(old, new, steps.to_s.split)
