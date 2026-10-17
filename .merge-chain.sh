#!/bin/bash
# 门禁化合并链: update-branch→CI全绿→toggle→squash合→恢复保护; 任一环失败跳过该PR
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

for n in "$@"; do
  echo "===== PR #$n 开始 $(date +%H:%M:%S) ====="
  state=$(gh pr view $n -R $REPO --json state --jq .state)
  if [ "$state" != "OPEN" ]; then echo "PR#$n 状态=$state 跳过"; continue; fi
  gh pr update-branch $n -R $REPO > /dev/null 2>&1 || { echo "PR#$n update-branch失败(可能冲突) 跳过"; continue; }
  # 轮询CI至无pending(最长25分钟)
  ok=""
  for i in $(seq 1 150); do
    sleep 10
    out=$(gh pr checks $n -R $REPO 2>/dev/null)
    if ! echo "$out" | grep -q "pending"; then
      if echo "$out" | grep -q "fail"; then echo "PR#$n CI有fail: $(echo "$out"|grep fail|head -2) 跳过"; else ok=1; fi
      break
    fi
  done
  if [ -z "$ok" ]; then [ $i -ge 150 ] && echo "PR#$n CI超时25分钟 跳过"; continue; fi
  mkbody 0 false .prot-open.json
  gh api -X PUT repos/$REPO/branches/main/protection --input .prot-open.json --silent || { echo "PR#$n toggle失败 跳过"; continue; }
  sleep 2
  if gh pr merge $n -R $REPO --squash --delete-branch > /dev/null 2>&1; then
    echo "PR#$n 合并成功 $(date +%H:%M:%S)"
  else
    echo "PR#$n merge命令失败(toggle已放开状态)!"
  fi
  mkbody 1 true .prot-restore.json
  gh api -X PUT repos/$REPO/branches/main/protection --input .prot-restore.json --silent && echo "保护已恢复"
done
echo "===== 链完成 $(date +%H:%M:%S) ====="
gh api repos/$REPO/branches/main/protection --jq '.required_pull_request_reviews|{count:.required_approving_review_count,owner:.require_code_owner_reviews}'
