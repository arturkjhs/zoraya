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
require 'open3'
require 'tempfile'

def die(msg)
  warn "ci-makefile-steps: #{msg}"
  exit 2
end

class Mk
  attr_reader :owner, :vars, :rules

  CONDITIONAL = /\A\s*(ifeq|ifneq|ifdef|ifndef|else|endif)\b/
  ASSIGN = /\A\s*(?:(?:export|override|private)\s+)*([A-Za-z_][\w.\-]*)\s*(::=|:=|\?=|\+=|!=|=)(.*)\z/m

  def initialize(text)
    lines = text.lines.map(&:chomp)
    @owner = Array.new(lines.size, :ignore)
    @vars = Hash.new { |h, k| h[k] = [] }
    @rules = [] # { targets: [raw words], prereq: String, text: String }
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
        if first.strip.start_with?('#')
          mark.call(:ignore)
        elsif cur
          @rules[cur][:text] << ' ' << stmt.join(' ')
          mark.call([:r, cur])
        else
          mark.call(:unknown)
        end
      elsif first =~ CONDITIONAL
        mark.call(:unknown)
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
      return { targets: words(expand(targets)), prereq: prereq, text: "#{prereq} #{recipe}".dup }
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

  def refs(text)
    t = text.gsub('$$', '')
    r = t.scan(/\$[({]([A-Za-z_][\w.\-]*)[)}]/).flatten
    r += t.scan(/\$[({](?:call|origin|value|flavor)\s+([A-Za-z_][\w.\-]*)/).flatten
    r.uniq
  end

  # Targets a recipe runs through make: $(MAKE) [-f X] T..., a word with $$ is a prefix.
  def makes(text)
    out = []
    text.scan(/(?:\$\(MAKE\)|\bmake)\b([^;&|\n]*)/) do |(args)|
      toks = args.split
      skip = false
      toks.each do |w|
        if skip then skip = false; next end
        if w == '-f' || w == '-C' then skip = true; next end
        next if w.start_with?('-') || w.include?('=') && !w.include?('$$')
        w = w.gsub(/\A["']|["']\z/, '')
        out << (w.include?('$$') ? "#{w.split('$$').first}$(?)" : w)
      end
    end
    out
  end

  def rules_for(word)
    if word.include?('$(')
      pre = word[0...word.index('$(')]
      return [] if pre.empty?
      return @rules.each_index.select { |i| @rules[i][:targets].any? { |t| t.start_with?(pre) } }
    end
    @rules.each_index.select do |i|
      @rules[i][:targets].any? do |t|
        t == word || (t.include?('%') && Regexp.new("\\A#{Regexp.escape(t).sub('%', '.+')}\\z").match?(word))
      end
    end
  end

  # The nodes reached from the rules of a target: [:r, i] and [:v, name].
  def closure(target)
    seen = {}
    todo = rules_for(target).map { |i| [:r, i] }
    until todo.empty?
      n = todo.pop
      next if seen[n]
      seen[n] = true
      if n[0] == :r
        rule = @rules[n[1]]
        words(expand(rule[:prereq])).each { |w| todo.concat(rules_for(w).map { |i| [:r, i] }) }
        makes(rule[:text]).each { |w| todo.concat(rules_for(w).map { |i| [:r, i] }) }
        (refs(rule[:text]) + refs(rule[:prereq])).uniq.each { |v| todo << [:v, v] }
      else
        @vars[n[1]].each { |val| refs(val).each { |v| todo << [:v, v] } }
      end
    end
    seen
  end
end

def hunks(old, new)
  out, st = Open3.capture2('diff', '-U0', old, new)
  die('diff failed') unless st.exitstatus <= 1
  olds = []
  news = []
  out.each_line do |l|
    next unless (m = /\A@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.match(l))
    a, b, c, d = m[1].to_i, (m[2] || '1').to_i, m[3].to_i, (m[4] || '1').to_i
    olds.concat((a...(a + b)).to_a)
    news.concat((c...(c + d)).to_a)
  end
  [olds, news]
end

def main(old, new, steps)
  mo = Mk.new(File.read(old))
  mn = Mk.new(File.read(new))
  olds, news = hunks(old, new)
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
  if full
    warn "ci-makefile-steps: #{full}: every step"
    puts 'full'
    return
  end
  got = []
  unit = false
  build = false
  units.uniq.each do |m, node|
    hit = false
    steps.each do |s|
      next unless m.closure(s).key?(node)
      got << s
      hit = true
    end
    if m.closure('test-unit').key?(node)
      unit = true
      hit = true
    end
    if !hit && m.closure('ci-build').key?(node)
      build = true
    end
    what = node[0] == :v ? "variable #{node[1]}" : "rule #{m.rules[node[1]][:targets].join(' ')}"
    warn "ci-makefile-steps: #{what}: #{hit ? 'reaches steps' : 'reaches no step'}#{build && !hit ? ', but the build' : ''}"
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
