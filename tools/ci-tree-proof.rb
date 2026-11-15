#!/usr/bin/env ruby
# Proves, from the GitHub API and git alone, that a commit has been tested by
# the FULL set already: some run of the workflow `make test` went green on a
# commit with the same tree, with every job of the full set present and
# successful (docs/adr/0345-one-full-run-per-tree.md). Nothing is taken from a
# tag name, a branch name, a message or the run's trigger.
#
#   tools/ci-tree-proof.rb        (environment below)
#
#   SHA          the commit (default HEAD). The job names expected are read
#                from `git show SHA:tools/ci-tests.txt` and
#                `git show SHA:.github/workflows/test.yml`: the files of the
#                tree asked about, not of the checkout.
#   BEFORE       main's head before the push: SHA must be a merge commit with
#                exactly two parents, the first being BEFORE
#                (CI_TREE_PROOF_ANY_COMMIT=1: not required, for a person
#                asking about a commit by hand)
#   GITHUB_REPOSITORY  owner/name
#   RUN_ID       this run, never its own proof
#   CI_API       the command that answers an API path with JSON (default
#                `gh api`); it exits non-zero on any error
#   CI_TREE_PROOF_CALL_TIMEOUT (30 s) and CI_TREE_PROOF_TIMEOUT (120 s): a
#                deadline for each API call and for the whole proof; a call
#                that is not answered in time is an error (the full set), never
#                a plan job that hangs until its own timeout
#
# Proven: prints `proof=1`, `proof_run=ID`, `proof_tree=TREE` and exits 0.
# Anything else, including every error of the API (rate limit, network), exits
# 1 with the reason on stderr: the caller then runs the full set (fail safe).
# Plain Ruby, no gems; the same on 2.6 (a Mac) and 3.2 (CI's image).
require 'json'
require 'shellwords'

def no(why)
  warn "ci-tree-proof: not proven: #{why}"
  exit 1
end

def now
  Process.clock_gettime(Process::CLOCK_MONOTONIC)
end

def git(*args)
  out = IO.popen(['git', *args, { err: File::NULL }], &:read)
  $?.success? ? out.strip : nil
end

CALL_LIMIT = (ENV['CI_TREE_PROOF_CALL_TIMEOUT'] || 30).to_f
STOP_AT = now + (ENV['CI_TREE_PROOF_TIMEOUT'] || 120).to_f

# The command's output, or nil: it failed, or did not finish within the
# limit (its process group is killed). The limit covers the reading and the
# wait for the exit: a command that closes its stdout and goes on running is
# stopped too.
def capture(cmd, limit)
  rd, wr = IO.pipe
  pid = Process.spawn(cmd, out: wr, err: File::NULL, in: File::NULL, pgroup: true)
  wr.close
  stop = now + limit
  buf = ''
  timed_out = false
  begin
    loop do
      left = stop - now
      if left <= 0 || IO.select([rd], nil, nil, left).nil?
        timed_out = true
        break
      end
      begin
        buf << rd.read_nonblock(65_536)
      rescue IO::WaitReadable
        next
      end
    end
  rescue EOFError
    nil
  end
  until timed_out || Process.wait(pid, Process::WNOHANG)
    timed_out = now >= stop
    sleep 0.05 unless timed_out
  end
  if timed_out
    begin
      Process.kill('KILL', -pid)
    rescue StandardError
      nil
    end
    begin
      Process.wait(pid)
    rescue StandardError
      nil
    end
    warn "ci-tree-proof: a call did not answer within #{limit.round(1)} s"
    return nil
  end
  $?.success? ? buf : nil
end

def api(path)
  limit = [CALL_LIMIT, STOP_AT - now].min
  return nil if limit <= 0
  out = capture("#{ENV['CI_API'] || 'gh api'} #{Shellwords.escape(path)}", limit)
  out && JSON.parse(out)
rescue StandardError
  nil
end

require 'timeout'

sha = ENV['SHA'] || 'HEAD'
sha = git('rev-parse', '--verify', "#{sha}^{commit}") or no('commit unknown')
before = ENV['BEFORE'].to_s
repo = ENV['GITHUB_REPOSITORY'].to_s
no('no GITHUB_REPOSITORY') if repo.empty?

parents = (git('rev-list', '--parents', '-n', '1', sha) || '').split[1..-1] || []
if ENV['CI_TREE_PROOF_ANY_COMMIT'] != '1'
  no("#{sha} has #{parents.size} parents, not exactly two") unless parents.size == 2
  no("first parent #{parents[0]} is not the previous head #{before.empty? ? '(none)' : before}") unless parents[0] == before
end

tree = git('rev-parse', "#{sha}^{tree}") or no('no tree')

# The jobs a full run has, from the files of SHA's own tree: every line of the
# full list, `build`, and every command of the checks matrix. A job skipped or
# missing is a plan that was not the full set (or a docs-only plan).
list = git('show', "#{sha}:tools/ci-tests.txt") or no('tools/ci-tests.txt cannot be read at the commit')
workflow = git('show', "#{sha}:.github/workflows/test.yml") or no('test.yml cannot be read at the commit')
tests = list.lines.map(&:strip).reject { |l| l.empty? || l.start_with?('#') }
checks = workflow.lines.map { |l| l =~ /^\s*- (make .*?)\s*$/ ? $1 : nil }.compact
no('the full list is empty') if tests.empty?
no('the checks matrix is empty') if checks.empty?
expected = (['build'] + tests + checks).uniq

# What a job must show: the step that runs its command succeeded, so that a
# job whose step was skipped (a docs-only plan's checks) does not count.
def step_of(name)
  name == 'build' ? 'make ci-build' : 'Run'
end

# The runs to look at: first those on the merge's second parent (the
# candidate of the documented flow) and on the commit itself, found by
# head_sha whatever their age, then the 100 latest successful ones.
heads = (parents[1..-1] || []) + [sha]
runs = []
heads.uniq.each do |h|
  r = api("repos/#{repo}/actions/workflows/test.yml/runs?head_sha=#{h}&status=success&per_page=100")
  runs.concat(r['workflow_runs']) if r && r['workflow_runs'].is_a?(Array)
end
scan = api("repos/#{repo}/actions/workflows/test.yml/runs?per_page=100&status=success")
runs.concat(scan['workflow_runs']) if scan && scan['workflow_runs'].is_a?(Array)
no('no list of runs can be read') if runs.empty? && scan.nil?

tree_of = {}
seen = {}
runs.each do |r|
  next unless r.is_a?(Hash)
  next if seen[r['id']]
  seen[r['id']] = true
  next unless r['conclusion'] == 'success'
  next unless %w[push workflow_dispatch schedule].include?(r['event'])
  next if r['id'].to_s == ENV['RUN_ID'].to_s
  head = r['head_sha'].to_s
  next unless head =~ /\A[0-9a-f]{40}\z/
  t = (tree_of[head] ||= (git('rev-parse', "#{head}^{tree}") ||
                          ((c = api("repos/#{repo}/git/commits/#{head}")) && c['tree'] && c['tree']['sha']) || ''))
  next unless t == tree

  ok = {}
  page = 1
  loop do
    j = api("repos/#{repo}/actions/runs/#{r['id']}/jobs?per_page=100&page=#{page}")
    break unless j && j['jobs'].is_a?(Array)
    j['jobs'].each do |x|
      n = x['name']
      step = (x['steps'] || []).find { |s| s['name'] == step_of(n) }
      good = x['conclusion'] == 'success' && !step.nil? && step['conclusion'] == 'success'
      ok[n] = ok.fetch(n, true) && good
    end
    break if ok.size >= j['total_count'].to_i || j['jobs'].empty? || page >= 10
    page += 1
  end
  bad = expected.reject { |n| ok[n] == true }
  if bad.empty?
    puts 'proof=1'
    puts "proof_run=#{r['id']}"
    puts "proof_tree=#{tree}"
    warn "ci-tree-proof: run #{r['id']} (#{r['event']}, commit #{head}) went green with all #{expected.size} jobs on tree #{tree}"
    exit 0
  end
  warn "ci-tree-proof: run #{r['id']} has tree #{tree} but not the full set (#{bad.size} jobs missing or not successful, first: #{bad.first})"
end
no("no green full run on tree #{tree}")
