#!/bin/sh
# Run every test_*.sh in this directory. Collects pass/fail counts and
# returns 0 only if all tests passed.
#
# Usage:
#   ./tests/run.sh                # run all tests
#   ./tests/run.sh -v             # verbose (don't truncate per-test output)
#
# Per-test timeouts are enforced by common.sh (QEMU_TIMEOUT env var,
# default 30s). Each test runs in isolation: its own kernel build, its
# own QEMU boot, its own serial log.
#
# To add a new E2E test:
#   1. Write the user program in tests/fixtures/<name>.rs
#   2. Call prepare_test_disk with that fixture from the test script
#   3. Drive QEMU and assert an observable result
#   4. ./tests/run.sh picks it up automatically.

set -u

TESTS_DIR=$(cd "$(dirname "$0")" && pwd)
REPO_ROOT=$(cd "$TESTS_DIR/.." && pwd)

VERBOSE=0
if [ "${1:-}" = "-v" ]; then
    VERBOSE=1
fi

cd "$REPO_ROOT"
sh scripts/lint.sh || exit $?

# Discover test scripts.
tests=$(ls "$TESTS_DIR"/test_*.sh 2>/dev/null | sort)
if [ -z "$tests" ]; then
    echo "[run] no tests found in $TESTS_DIR" >&2
    exit 1
fi

total=0
passed=0
failed_tests=""

for test in $tests; do
    name=$(basename "$test" .sh)
    total=$((total + 1))

    if [ $VERBOSE -eq 1 ]; then
        echo ""
        echo "=========================================="
        echo "=== RUN $name"
        echo "=========================================="
        if sh "$test"; then
            passed=$((passed + 1))
            echo "=== PASS $name"
        else
            failed_tests="$failed_tests $name"
            echo "=== FAIL $name"
        fi
    else
        # Quiet mode: collapse per-test output unless it fails.
        printf "%-30s " "$name"
        test_log=$(mktemp /tmp/mini-os-run.XXXXXX)
        if sh "$test" > "$test_log" 2>&1; then
            passed=$((passed + 1))
            echo "PASS"
            rm "$test_log"
        else
            failed_tests="$failed_tests $name"
            echo "FAIL — log at $test_log"
        fi
    fi
done

echo ""
echo "=== SUMMARY ==="
echo "Passed: $passed / $total"
if [ -n "$failed_tests" ]; then
    echo "Failed:$failed_tests"
fi

[ "$passed" -eq "$total" ]
