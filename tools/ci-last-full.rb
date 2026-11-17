#!/usr/bin/env ruby
# The scheduled run of the full set (ADR-0348, items 3 and 4): reads from the
# GitHub API the latest scheduled runs of the workflow `make test` on main that
# really ran the full set (the job `build` ran: not skipped, so a run that found
# nothing new on main does not count), and answers
#
#   tools/ci-last-full.rb           environment below; prints
#       skip=1        the latest such run, green or red, was on this very commit:
#                     nothing new on main, the 2-hourly run need not run again
#       skip=0
#       last_conclusion=success|failure   how the latest such run ended (a skipping 2-hourly run
#                     fails when this is `failure`, so that it carries the red forward)
#       last_run=ID last_sha=SHA        the latest such run (if any)
#       green_run=ID green_sha=SHA      the latest such run that went green (if any)
#   tools/ci-last-full.rb --green-for SHA   the stage-tag rule: prints the latest scheduled run whose
#       `build` job ran, that went green and whose head has SHA as an ancestor (SHA itself counts):
#       `green_run=ID green_sha=HEAD_SHA`; exits 1 if there is none (or the API fails). A run that
#       skipped, or a red one, never counts.
#   tools/ci-last-full.rb --merges SHA   the commits of main's first-parent line since SHA
#       (git log --first-parent SHA..HEAD: merges and direct commits, merges marked), one a
#       line; with no SHA, or one this checkout does not have, the 15 latest commits, said so.
#
#   GITHUB_REPOSITORY  owner/name     SHA   main's head (default HEAD)
#   RUN_ID             this run, never its own predecessor
#   CI_API             the command that answers an API path with JSON (default
#                      `gh api`); it exits non-zero on any error
#
# Fail safe: any error (API, rate limit, a missing answer) exits 1 with the reason
# on stderr, and the caller runs the full set: unknown means run.
require 'json'
require 'open3'
require 'timeout'

def no(why)
  warn "ci-last-full: #{why}"
  exit 1
end

def api(path)
  cmd = (ENV['CI_API'] || 'gh api').split + [path]
  out = nil
  begin
    Timeout.timeout(Integer(ENV['CI_LAST_FULL_CALL_TIMEOUT'] || 30)) do
      out, st = Open3.capture2(*cmd)
      no("#{path}: the API answered an error") unless st.success?
    end
  rescue Timeout::Error
    no("#{path}: the API did not answer in time")
  end
  JSON.parse(out)
rescue JSON::ParserError
  no("#{path}: the answer is not JSON")
end

def git(*a)
  out, st = Open3.capture2('git', *a)
  st.success? ? out : nil
end

def commits(*range)
  out = git('log', '--first-parent', '--format=%h|%p|%s', *range).to_s
  out.lines.map do |l|
    h, p, subj = l.chomp.split('|', 3)
    "#{h} #{p.to_s.split.size > 1 ? '[merge] ' : ''}#{subj}"
  end.join("\n")
end

if ARGV[0] == '--merges'
  last = ARGV[1].to_s
  if !last.empty? && git('rev-parse', '-q', '--verify', "#{last}^{commit}")
    puts "Commits and merges on main since the last green full run (#{last[0, 10]}):"
    list = commits("#{last}..HEAD")
    puts(list.empty? ? '(none)' : list)
  else
    puts 'No green full run is known (or its commit is not in this checkout): the 15 latest commits on main:'
    puts commits('-15')
  end
  exit 0
end

repo = ENV['GITHUB_REPOSITORY'] or no('GITHUB_REPOSITORY is not set')
sha = ENV['SHA'] || git('rev-parse', 'HEAD').to_s.strip
no('no commit') if sha.empty?
this = ENV['RUN_ID'].to_s

runs = api("repos/#{repo}/actions/workflows/test.yml/runs?event=schedule&branch=main&status=completed&per_page=100")['workflow_runs']
no('no list of runs') unless runs.is_a?(Array)

# Each completed scheduled run that really ran the full set (its build job ran), newest first.
def each_full_run(runs, repo, this)
  examined = 0
  runs.each do |r|
    next if r['id'].to_s == this
    next unless %w[success failure].include?(r['conclusion'])
    break if examined >= 12
    examined += 1
    jobs = api("repos/#{repo}/actions/runs/#{r['id']}/jobs?per_page=100")['jobs']
    no("run #{r['id']}: no list of jobs") unless jobs.is_a?(Array)
    build = jobs.find { |j| j['name'] == 'build' }
    next unless build && %w[success failure].include?(build['conclusion']) # a skipped build: not a full run
    yield r
  end
end

if ARGV[0] == '--green-for'
  want = ARGV[1].to_s
  no('--green-for needs a commit') if want.empty?
  found = nil
  each_full_run(runs, repo, this) do |r|
    next unless r['conclusion'] == 'success'
    # The run's head may not be in this checkout: fetch it; a failed fetch means it does not count (fail safe).
    git('fetch', '-q', 'origin', r['head_sha']) unless git('cat-file', '-e', "#{r['head_sha']}^{commit}")
    next unless git('merge-base', '--is-ancestor', want, r['head_sha']) # unknown commits do not count
    found = r
    break
  end
  no("no green full scheduled run includes #{want}") unless found
  puts "green_run=#{found['id']}\ngreen_sha=#{found['head_sha']}"
  exit 0
end

last = nil
green = nil
each_full_run(runs, repo, this) do |r|
  last ||= r
  green ||= r if r['conclusion'] == 'success'
  break if last && green
end

puts "skip=#{last && last['head_sha'] == sha ? 1 : 0}"
puts "last_run=#{last['id']}\nlast_sha=#{last['head_sha']}\nlast_conclusion=#{last['conclusion']}" if last
puts "green_run=#{green['id']}\ngreen_sha=#{green['head_sha']}" if green
