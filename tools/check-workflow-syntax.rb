#!/usr/bin/env ruby
# Fails when a GitHub workflow file would be rejected for a syntax slip that a
# YAML parse alone does not see (TASK-31 part ci-expr: twice a missing comma
# in `hashFiles('a', 'b' 'c')` made GitHub refuse the whole workflow, a run
# with zero jobs, and the file was still valid YAML).
#
#   tools/check-workflow-syntax.rb FILE...     check the files
#   tools/check-workflow-syntax.rb --selftest  the checker's own tests
#
# Per file: (a) it parses as YAML (ruby's psych: the CI image installs ruby for
# this, .github/ci/Dockerfile; macOS has it); (b) no line is indented with a
# tab; no mapping has a key twice; (c) every `${{ ... }}` is closed, and inside
# it parentheses, brackets and quotes balance and every call's arguments are
# separated by exactly one comma (no two operands side by side, no leading,
# doubled or trailing comma); the same for a `hashFiles(...)` call written
# outside `${{ }}` (a `run:` line). This is a conservative structural check,
# not GitHub's grammar: it knows no function names and no types.
require 'yaml'

# Tokenises an expression starting at +pos+ of +text+. mode :expr ends at `}}`
# outside a string at depth 0 (the text after `${{`); mode :call ends when the
# paren opened at +pos+ closes. Returns [end_pos, errors], errors being
# [offset, message]; end_pos is nil when it never ended.
def scan_expr(text, pos, mode)
  errors = []
  # Each frame: kind (:top, :paren, :bracket), prev token kind, open offset.
  # prev: :open, :operand, :operator, :comma, :ident (a name that may be a call).
  stack = [{ kind: (mode == :call ? :paren : :top), prev: :open, at: pos }]
  i = pos
  n = text.length
  err = lambda { |off, msg| errors << [off, msg] }
  operand = lambda do |off, what|
    f = stack.last
    if f[:prev] == :operand || f[:prev] == :ident
      err.call(off, "two values side by side ('#{what}' follows a value): a comma or an operator is missing")
    end
  end
  while i < n
    c = text[i]
    if c =~ /\s/
      i += 1
    elsif c == "'"
      start = i
      i += 1
      closed = false
      while i < n
        if text[i] == "'"
          if text[i + 1] == "'"
            i += 2
            next
          end
          closed = true
          i += 1
          break
        end
        i += 1
      end
      unless closed
        err.call(start, "unterminated string literal")
        return [nil, errors]
      end
      operand.call(start, "string")
      stack.last[:prev] = :operand
    elsif c == '('
      f = stack.last
      # A name followed by `(` is a call; anything else is a grouping.
      err.call(i, "'(' follows a value") if f[:prev] == :operand
      f[:prev] = :open_pending
      stack.push({ kind: :paren, prev: :open, at: i })
      i += 1
      next
    elsif c == '['
      f = stack.last
      err.call(i, "'[' with nothing to index") unless f[:prev] == :operand || f[:prev] == :ident
      stack.push({ kind: :bracket, prev: :open, at: i })
      i += 1
      next
    elsif c == ')' || c == ']'
      f = stack.last
      want = c == ')' ? :paren : :bracket
      if f[:kind] != want
        err.call(i, "unbalanced '#{c}'")
        return [nil, errors] if stack.length == 1
        i += 1
        next
      end
      if f[:prev] == :comma
        err.call(i, "trailing comma before '#{c}'")
      elsif f[:prev] == :operator
        err.call(i, "operator with no right-hand value before '#{c}'")
      end
      stack.pop
      if mode == :call && stack.empty?
        return [i + 1, errors]
      end
      stack.last[:prev] = :operand
      i += 1
      next
    elsif c == ','
      f = stack.last
      if f[:kind] == :top
        err.call(i, "comma outside any call")
      elsif f[:prev] == :open
        err.call(i, "leading comma")
      elsif f[:prev] == :comma
        err.call(i, "two commas in a row")
      elsif f[:prev] == :operator
        err.call(i, "operator with no right-hand value before ','")
      end
      f[:prev] = :comma
      i += 1
      next
    elsif c == '}' && text[i + 1] == '}' && mode == :expr && stack.length == 1
      f = stack.last
      err.call(i, "operator with no right-hand value before '}}'") if f[:prev] == :operator
      err.call(i, "empty expression") if f[:prev] == :open
      return [i + 2, errors]
    elsif c =~ /[!&|=<>]/
      j = i
      j += 1 while j < n && text[j] =~ /[!&|=<>]/
      op = text[i...j]
      if op !~ /\A(!|&&|\|\||==|!=|<|>|<=|>=)\z/
        err.call(i, "unknown operator '#{op}'")
      end
      f = stack.last
      if op == '!'
        err.call(i, "'!' follows a value") if f[:prev] == :operand || f[:prev] == :ident
        f[:prev] = :operator
      else
        if f[:prev] != :operand && f[:prev] != :ident
          err.call(i, "operator '#{op}' with no left-hand value")
        end
        f[:prev] = :operator
      end
      i = j
      next
    elsif c =~ /[A-Za-z0-9_.*\-]/
      j = i
      j += 1 while j < n && text[j] =~ /[A-Za-z0-9_.*\-]/
      word = text[i...j]
      operand.call(i, word)
      # A name directly followed by `(` is a call: keep :ident so `(` is allowed.
      k = j
      k += 1 while k < n && text[k] =~ /[ \t]/
      stack.last[:prev] = (text[k] == '(' ? :callname : :operand)
      i = j
      next
    else
      err.call(i, "unexpected character '#{c}'")
      i += 1
      next
    end
    # (string branch falls through to here)
  end
  # Ran out of text.
  [nil, errors]
end

# `(` after a call name must not count as "follows a value".
# (Handled above by :callname being neither :operand nor :ident.)

def line_of(text, off)
  text[0, off].count("\n") + 1
end

# Returns the list of "FILE:LINE: message" for one workflow.
def check_workflow(path, text)
  out = []
  add = lambda { |line, msg| out << "#{path}:#{line}: #{msg}" }

  # (a) YAML parse; (b) duplicate keys.
  begin
    stream = Psych.parse_stream(text)
    walk = nil
    walk = lambda do |node|
      if node.is_a?(Psych::Nodes::Mapping)
        seen = {}
        node.children.each_slice(2) do |k, _v|
          next unless k.is_a?(Psych::Nodes::Scalar)
          if seen.key?(k.value)
            add.call(k.start_line + 1, "duplicate key '#{k.value}' (first on line #{seen[k.value] + 1})")
          else
            seen[k.value] = k.start_line
          end
        end
      end
      (node.respond_to?(:children) ? node.children || [] : []).each { |c| walk.call(c) }
    end
    walk.call(stream)
  rescue Psych::SyntaxError => e
    add.call(e.line || 1, "not valid YAML: #{e.problem} #{e.context}".strip)
    return out
  end

  # (b) tab indentation.
  text.each_line.with_index(1) do |l, no|
    add.call(no, "indented with a tab") if l =~ /\A[ ]*\t/
  end

  # (c) expressions. Whole-line comments are skipped.
  masked = text.each_line.map { |l| l =~ /\A\s*#/ ? l.gsub(/[^\n]/, ' ') : l }.join
  i = 0
  while i < masked.length
    s = masked.index('${{', i)
    h = masked.index('hashFiles(', i)
    break if s.nil? && h.nil?
    if h && (s.nil? || h < s)
      endp, errs = scan_expr(masked, h + 'hashFiles('.length, :call)
      errs.each { |off, m| add.call(line_of(masked, off), "hashFiles(...): #{m}") }
      add.call(line_of(masked, h), "hashFiles( is never closed") if endp.nil?
      i = endp || (h + 1)
    else
      endp, errs = scan_expr(masked, s + 3, :expr)
      errs.each { |off, m| add.call(line_of(masked, off), "in ${{ }}: #{m}") }
      if endp.nil?
        add.call(line_of(masked, s), "unbalanced `${{`: no closing `}}`")
        i = s + 3
      else
        i = endp
      end
    end
  end
  out
end

def selftest
  fails = []
  expect = lambda do |name, yaml, pattern|
    r = check_workflow('t.yml', yaml)
    if pattern.nil?
      fails << "#{name}: expected no error, got #{r.inspect}" unless r.empty?
    else
      fails << "#{name}: expected an error matching #{pattern.inspect}, got #{r.inspect}" unless r.any? { |m| m =~ pattern }
    end
  end
  w = lambda { |expr| "jobs:\n  a:\n    steps:\n      - run: x\n        with:\n          key: #{expr}\n" }
  # The historical bad case (twice rejected by GitHub).
  expect.call('missing comma', w.call("${{ hashFiles('docs/adr/0398-*.md' 'docs/adr/0389-*.md') }}"), /t\.yml:6: .*side by side/)
  expect.call('missing comma, three', w.call("${{ hashFiles('a', 'b' 'c') }}"), /side by side/)
  expect.call('trailing comma', w.call("${{ hashFiles('a', 'b',) }}"), /trailing comma/)
  expect.call('leading comma', w.call("${{ hashFiles(, 'a') }}"), /leading comma/)
  expect.call('double comma', w.call("${{ hashFiles('a',, 'b') }}"), /two commas/)
  expect.call('unbalanced open', w.call("${{ hashFiles('a') "), /unbalanced `\$\{\{`/)
  expect.call('unbalanced paren', w.call("${{ hashFiles('a' }}"), /unbalanced/)
  expect.call('unterminated string', w.call("${{ format('a }}"), /unterminated|unbalanced/)
  expect.call('missing comma other call', w.call("${{ format('{0}' github.sha) }}"), /side by side/)
  expect.call('operator dangling', w.call("${{ a && }}"), /no right-hand/)
  expect.call('hashFiles in run line', "jobs:\n  a:\n    steps:\n      - run: echo hashFiles('a' 'b')\n", /hashFiles.*side by side/)
  expect.call('trailing comma in run line', "jobs:\n  a:\n    steps:\n      - run: echo hashFiles('a',)\n", /trailing comma/)
  expect.call('tab', "jobs:\n\ta: 1\n", /tab|not valid YAML/)
  expect.call('tab in a block scalar', "jobs:\n  a:\n    run: |\n\techo\n", /tab/)
  expect.call('duplicate key', "on: push\non: pull_request\n", /duplicate key 'on'.*line 1/)
  expect.call('duplicate nested key', "jobs:\n  a:\n    name: x\n    name: y\n", /duplicate key 'name'/)
  expect.call('not YAML', "a: [1, 2\nb: c\n", /not valid YAML/)
  # Good text.
  expect.call('good hashFiles', w.call("${{ hashFiles('a', 'b') }}"), nil)
  expect.call('good nested', w.call("${{ needs.x.outputs.y != '' && format('{0}-{1}', 'a', hashFiles('p/**', 'q')) || 'bare' }}"), nil)
  expect.call('good braces in a string', w.call("${{ format('{{\"image\":\"{0}\"}}', a.b) }}"), nil)
  expect.call('good quote escape', w.call("${{ format('it''s {0}', a) }}"), nil)
  expect.call('good index and not', w.call("${{ !cancelled() && matrix.os[0] == 'x' && contains(a.b, 'c') }}"), nil)
  expect.call('good empty call', w.call("${{ always() }}"), nil)
  expect.call('good comment', "# a ${{ unbalanced in a comment\non: push\n", nil)
  # The files of this repository.
  Dir.glob('.github/workflows/*.yml').sort.each do |f|
    r = check_workflow(f, File.read(f))
    fails << "#{f}: expected no error, got #{r.inspect}" unless r.empty?
  end
  if fails.empty?
    puts 'check-workflow-syntax: selftest passed'
    0
  else
    fails.each { |m| warn "check-workflow-syntax: selftest: #{m}" }
    1
  end
end

if ARGV == ['--selftest']
  exit selftest
elsif ARGV.empty?
  warn 'usage: check-workflow-syntax.rb FILE... | --selftest'
  exit 2
else
  errors = []
  ARGV.each do |f|
    unless File.readable?(f)
      warn "check-workflow-syntax: cannot read #{f}"
      exit 2
    end
    errors.concat(check_workflow(f, File.read(f)))
  end
  if errors.empty?
    puts "check-workflow-syntax: #{ARGV.length} workflow file(s) parse, and their expressions are well formed"
  else
    errors.each { |m| warn "check-workflow-syntax: #{m}" }
    warn "check-workflow-syntax: #{errors.length} error(s)"
    exit 1
  end
end
