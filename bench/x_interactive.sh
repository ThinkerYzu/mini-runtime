#!/bin/bash
# Open X in the mini runtime's bridge tab and use it by hand, with or without the API process.
#
# The page runs in `realsite` on the real clock (INTERACTIVE=1); the release Firefox shows it in
# the bridge tab, logged in through the test profile (~/.cache/mrt-realsites-profile, task 0e920).
# Close the tab or its window to end the run; the runtime is then measured and the results written.
#
# usage: bench/x_interactive.sh [--api | --tab] [--replay] [--seconds N] [--actions FILE] [--xvfb]
#   --tab          the bridge tab answers every API the page calls (the default)
#   --api          the page's crypto, storage, cookies, fetch, WebSockets and env go to mrt-api,
#                  the API process (API_PROCESS=all, SPEC decision 27); the tab keeps the rest
#   --replay       X's recording (realsites-replay-post, no network) instead of live X
#   --seconds N    stop N seconds after the load instead of when the tab is closed
#   --actions FILE WebDriver steps done in the tab (INPUT_ACTIONS, bench/input_actions.py)
#   --xvfb         run under xvfb-run, with nothing to see (checks; needs --seconds)
# Anything else in the environment (MRT_*, INTERACTIVE_DEBUG, MARIONETTE_PORT, ...) is passed on.
#
# Results: bench/mini_realsites_x_<mode>.json, logs and screenshots in bench/realsites-mini-x-<mode>/,
# where <mode> is tab or api, so runs of the two modes do not mix.
set -euo pipefail

BENCH=$(cd "$(dirname "$0")" && pwd)
ROOT=$(dirname "$BENCH")
GECKO=${GECKO:-$HOME/progm/firefox-hibernate}
FIREFOX=${FIREFOX_BIN:-$HOME/.cache/mrt-firefox-release/firefox/firefox}
REPLAY_POST=${REPLAY_POST:-$HOME/progm/mini-runtime/bench/realsites-replay-post}

mode=tab replay= seconds= actions= xvfb=
while [ $# -gt 0 ]; do
    case $1 in
        --api) mode=api ;;
        --tab) mode=tab ;;
        --replay) replay=1 ;;
        --seconds) seconds=$2; shift ;;
        --actions) actions=$(realpath "$2"); shift ;;
        --xvfb) xvfb=1 ;;
        -h|--help) sed -n '2,/^set -e/p' "$0" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

# What the run needs, checked now rather than after Firefox has started.
missing() { echo "missing: $1" >&2; exit 1; }
[ -x "$FIREFOX" ] || missing "release Firefox $FIREFOX (the test profile was last used by 157; set FIREFOX_BIN)"
[ -x "$GECKO/mach" ] || missing "$GECKO/mach (a Firefox tree for ./mach python; set GECKO)"
[ -x "$ROOT/mrt-js/target/release/realsite" ] || missing "realsite: cd $ROOT/mrt-js && cargo build --release"
if [ $mode = api ]; then
    [ -x "$ROOT/mrt-api/target/release/mrt-api" ] || missing "mrt-api: cd $ROOT/mrt-api && cargo build --release"
fi
if [ -n "$replay" ]; then
    [ -d "$REPLAY_POST/x" ] || missing "X's recording $REPLAY_POST/x (set REPLAY_POST)"
fi
if [ -n "$xvfb" ] && [ -z "$seconds" ]; then
    echo "--xvfb needs --seconds: nobody can close a tab nobody sees" >&2; exit 2
fi
if [ -z "$xvfb" ] && [ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    echo "no display to show the tab on: run from the desktop, or add --xvfb --seconds N" >&2; exit 2
fi

env_vars=(ONLY=x INTERACTIVE=1 FIREFOX_BIN="$FIREFOX"
          MINI_DIR="realsites-mini-x-$mode" MINI_OUT="mini_realsites_x_$mode.json")
if [ $mode = api ]; then
    env_vars+=(API_PROCESS=all)
else
    # A caller's API_PROCESS would quietly turn the tab run into an API run.
    unset API_PROCESS
fi
[ -n "$replay" ] && env_vars+=(REPLAY=replay REPLAY_DIR="$REPLAY_POST")
[ -n "$seconds" ] && env_vars+=(INTERACTIVE_S="$seconds")
[ -n "$actions" ] && env_vars+=(INPUT_ACTIONS="$actions")

run=(./mach python "$BENCH/mini_realsites.py")
[ -n "$xvfb" ] && run=(xvfb-run -a -s "-screen 0 1600x1000x24" "${run[@]}")

echo "X, $([ -n "$replay" ] && echo replayed || echo live), APIs answered by the $([ $mode = api ] && echo 'API process (mrt-api)' || echo 'bridge tab')"
[ -z "$seconds" ] && echo "Close the bridge tab or its window to end the run (Ctrl-C would end Firefox before the runtime is measured)."
cd "$GECKO"
env "${env_vars[@]}" "${run[@]}"

echo
echo "results: $BENCH/mini_realsites_x_$mode.json"
echo "log:     $BENCH/realsites-mini-x-$mode/x.log"
