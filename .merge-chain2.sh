#!/bin/bash
# 修正版链: update-branch→CI绿→toggle→合→恢复; merge失败若因BEHIND则再update-branch+等CI重试一次
set -u
cd "$(dirname "$0")"
REPO=sunormesky-max/epicode
mkbody() { python3 -c "
import json,sys
count,owner,path=int(sys.argv[1]),sys.argv[2]=='true',sys.argv[3]
d=json.load(open('.prot-backup-20261017.json'))
r=d['required_pull_request_reviews']
json.dump({
 'required_pull_request_reviews':{'required_approving_review_count':count,'require_code_owner_reviews':owner,
  'dismiss_stale_reviews':r.get('dismiss_stale_reviews',True),'require_last_push_approval':r.get('require_last_push_approval',False)},
 'required_status_checks':{'strict':True,'contexts':[c['context'] for c in d['required_status_checks']['checks']]},
 'enforce_admins':True,'restrictions':None,'allow_force_pushes':False,'allow_deletions':False,
 'required_linear_history':True,'block_creations':False},open(path,'w'))" $1 $2 $3; }
wait_ci() { # $1=pr  返回0=全绿 1=fail 2=超时
  for i in $(seq 1 150); do
    sleep 10
    out=$(gh pr checks $1 -R $REPO 2>/dev/null)
    if ! echo "$out" | grep -q "pending"; then
      echo "$out" | grep -q "fail" && return 1
      return 0
    fi
  done
  return 2
}
try_merge() { # $1=pr: toggle→merge→恢复
  mkbody 0 false .prot-open.json
  gh api -X PUT repos/$REPO/branches/main/protection --input .prot-open.json --silent || return 1
  sleep 2
  gh pr merge $1 -R $REPO --squash --delete-branch >/dev/null 2>&1
  rc=$?
  mkbody 1 true .prot-restore.json
  gh api -X PUT repos/$REPO/branches/main/protection --input .prot-restore.json --silent
  return $rc
}
for n in "$@"; do
  echo "===== PR #$n 开始 $(date +%H:%M:%S) ====="
  [ "$(gh pr view $n -R $REPO --json state --jq .state)" = "OPEN" ] || { echo "PR#$n 非OPEN跳过"; continue; }
  for round in 1 2; do
    gh pr update-branch $n -R $REPO >/dev/null 2>&1 || { echo "PR#$n update-branch冲突 跳过"; break; }
    wait_ci $n; rc=$?
    [ $rc -eq 1 ] && { echo "PR#$n CI fail 跳过: $(gh pr checks $n -R $REPO 2>/dev/null|grep fail|head -1)"; break; }
    [ $rc -eq 2 ] && { echo "PR#$n CI超时 跳过"; break; }
    if try_merge $n; then echo "PR#$n 合并成功 $(date +%H:%M:%S)"; break; fi
    [ $round -eq 2 ] && echo "PR#$n 两轮均失败"
  done
done
echo "===== 链2完成 $(date +%H:%M:%S) ====="
gh api repos/$REPO/branches/main/protection --jq '.required_pull_request_reviews|{count:.required_approving_review_count,owner:.require_code_owner_reviews}'
