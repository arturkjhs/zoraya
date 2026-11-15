#!/usr/bin/env ruby
# Proves, from the GitHub API and git alone, that a commit has been tested by
# the FULL set already: some run of the workflow `make test` went green on a
# commit with the same tree, with every job of the full set present and
# successful (docs/adr/0345-one-full-run-per-tree.md). Nothing is taken from a
# tag name, a branch name, a message or the run's trigger.
#
#   tools/ci-tree-proof.rb        (environment below)
#
#   SHA          the commit (default HEAD)
#   BEFORE       main's head before the push: SHA must be a merge commit with
#                exactly two parents, the first being BEFORE (MERGE=0: not
#                required, for a person asking about a commit by hand)
#   GITHUB_REPOSITORY  owner/name
#   RUN_ID       this run, never its own proof
#   CI_API       the command that answers an API path with JSON (default
#                `gh api`); it exits non-zero on any error
#   WORKFLOW, LIST   the workflow file and the full list (tools/ci-tests.txt)
#                of the checkout: the checkout has the proving tree's files, so
#                the job names expected are the proving run's
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

def git(*args)
  out = IO.popen(['git', *args, { err: File::NULL }], &:read)
  $?.success? ? out.strip : nil
end

def api(path)
  cmd = ENV['CI_API'] || 'gh api'
  out = IO.popen("#{cmd} #{Shellwords.escape(path)}", err: File::NULL, &:read)
  return nil unless $?.success?
  JSON.parse(out)
rescue StandardError
  nil
end

sha = ENV['SHA'] || 'HEAD'
sha = git('rev-parse', '--verify', "#{sha}^{commit}") or no('commit unknown')
before = ENV['BEFORE'].to_s
repo = ENV['GITHUB_REPOSITORY'].to_s
no('no GITHUB_REPOSITORY') if repo.empty?
workflow = ENV['WORKFLOW'] || '.github/workflows/test.yml'
list = ENV['LIST'] || 'tools/ci-tests.txt'

if ENV['MERGE'] != '0'
  parents = (git('rev-list', '--parents', '-n', '1', sha) || '').split[1..-1] || []
  no("#{sha} has #{parents.size} parents, not exactly two") unless parents.size == 2
  no("first parent #{parents[0]} is not the previous head #{before.empty? ? '(none)' : before}") unless parents[0] == before
end

tree = git('rev-parse', "#{sha}^{tree}") or no('no tree')

# The jobs a full run has: every line of the full list, `build`, and every
# command of the checks matrix. A job skipped or missing is a plan that was
# not the full set (or a docs-only plan).
expected = ['build']
File.readlines(list).each do |l|
  l = l.strip
  expected << l unless l.empty? || l.start_with?('#')
end
File.readlines(workflow).each do |l|
  expected << $1 if l =~ /^\s*- (make .*?)\s*$/
end
expected.uniq!
no('no jobs expected') if expected.size < 3

runs = api("repos/#{repo}/actions/workflows/test.yml/runs?per_page=100&status=success")
no('the list of runs cannot be read') unless runs && runs['workflow_runs'].is_a?(Array)

tree_of = {}
runs['workflow_runs'].each do |r|
  next unless r.is_a?(Hash)
  next unless r['conclusion'] == 'success'
  next unless %w[push workflow_dispatch schedule].include?(r['event'])
  next if r['id'].to_s == ENV['RUN_ID'].to_s
  head = r['head_sha'].to_s
  next unless head =~ /\A[0-9a-f]{40}\z/
  t = (tree_of[head] ||= (git('rev-parse', "#{head}^{tree}") ||
                          ((c = api("repos/#{repo}/git/commits/#{head}")) && c['tree'] && c['tree']['sha']) || ''))
  next unless t == tree

  names = {}
  page = 1
  loop do
    j = api("repos/#{repo}/actions/runs/#{r['id']}/jobs?per_page=100&page=#{page}")
    break unless j && j['jobs'].is_a?(Array)
    j['jobs'].each do |x|
      n = x['name']
      if x['conclusion'] == 'success' then names[n] ||= 'success' else names[n] = x['conclusion'].to_s end
    end
    break if names.size >= j['total_count'].to_i || j['jobs'].empty? || page >= 10
    page += 1
  end
  bad = expected.reject { |n| names[n] == 'success' }
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
