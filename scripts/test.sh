#!/usr/bin/env bash
#
# Test runner script for ApexShot
# Prevents OOM crashes by controlling parallelism and memory usage
#
# Usage:
#   ./scripts/test.sh              # Run all tests with safe defaults
#   ./scripts/test.sh --quick      # Run only unit tests (skip integration)
#   ./scripts/test.sh --lib        # Run only lib tests
#   ./scripts/test.sh --test NAME  # Run specific test
#   ./scripts/test.sh --watch      # Run tests with cargo-watch (requires cargo-watch)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default settings - conservative for memory safety
CARGO_JOBS="${CARGO_JOBS:-2}"
TEST_THREADS="${TEST_THREADS:-1}"
PROFILE="${PROFILE:-dev}"

# Parse arguments
MODE="all"
SPECIFIC_TEST=""
WATCH_MODE=false
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --quick)
            MODE="lib"
            shift
            ;;
        --lib)
            MODE="lib"
            shift
            ;;
        --test)
            MODE="specific"
            SPECIFIC_TEST="$2"
            shift 2
            ;;
        --watch)
            WATCH_MODE=true
            shift
            ;;
        --jobs)
            CARGO_JOBS="$2"
            shift 2
            ;;
        --threads)
            TEST_THREADS="$2"
            shift 2
            ;;
        --release)
            PROFILE="release"
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --quick, --lib    Run only unit tests (skip integration tests)"
            echo "  --test NAME       Run specific test by name"
            echo "  --watch           Run tests in watch mode (requires cargo-watch)"
            echo "  --jobs N          Set cargo compilation jobs (default: 2)"
            echo "  --threads N       Set test execution threads (default: 1)"
            echo "  --release         Build with release profile"
            echo "  --help, -h        Show this help message"
            exit 0
            ;;
        *)
            EXTRA_ARGS+=("$1")
            shift
            ;;
    esac
done

# Function to check memory usage
check_memory() {
    local available_kb
    available_kb=$(grep MemAvailable /proc/meminfo 2>/dev/null | awk '{print $2}') || true
    if [[ -n "$available_kb" ]]; then
        local available_mb=$((available_kb / 1024))
        if [[ $available_mb -lt 1024 ]]; then
            echo -e "${YELLOW}Warning: Low memory (${available_mb}MB available). Consider closing other applications.${NC}"
            return 1
        fi
    fi
    return 0
}

# Function to run tests with memory monitoring
run_tests() {
    local test_args=()
    
    # Build the cargo test command with memory-safe settings
    echo -e "${BLUE}Running tests with:${NC}"
    echo -e "  CARGO_JOBS=${CARGO_JOBS}"
    echo -e "  TEST_THREADS=${TEST_THREADS}"
    echo -e "  PROFILE=${PROFILE}"
    echo ""

    case $MODE in
        lib)
            echo -e "${BLUE}Running unit tests only (skipping integration tests)...${NC}"
            test_args=(--lib -- --test-threads="$TEST_THREADS")
            ;;
        specific)
            echo -e "${BLUE}Running specific test: ${SPECIFIC_TEST}${NC}"
            test_args=(-- "$SPECIFIC_TEST" --test-threads="$TEST_THREADS")
            ;;
        *)
            echo -e "${BLUE}Running all tests (unit + integration)...${NC}"
            test_args=(-- --test-threads="$TEST_THREADS")
            ;;
    esac

    # Add release flag if specified
    if [[ "$PROFILE" == "release" ]]; then
        test_args=(--release "${test_args[@]}")
    fi

    # Add any extra arguments
    test_args+=("${EXTRA_ARGS[@]}")

    # Run with memory check first
    check_memory || true

    echo ""
    echo -e "${BLUE}Command: cargo test --jobs ${CARGO_JOBS} ${test_args[*]}${NC}"
    echo ""

    # Export environment variables for the cargo process
    export CARGO_JOBS
    export RUST_TEST_THREADS="$TEST_THREADS"
    
    # Run the tests
    if cargo test --jobs "$CARGO_JOBS" "${test_args[@]}"; then
        echo ""
        echo -e "${GREEN}All tests passed!${NC}"
        return 0
    else
        echo ""
        echo -e "${RED}Some tests failed.${NC}"
        return 1
    fi
}

# Run in watch mode if requested
if [[ "$WATCH_MODE" == true ]]; then
    if ! command -v cargo-watch &> /dev/null; then
        echo -e "${YELLOW}cargo-watch not installed. Installing...${NC}"
        cargo install cargo-watch
    fi
    
    echo -e "${BLUE}Starting watch mode. Tests will re-run on file changes.${NC}"
    echo -e "${BLUE}Press Ctrl+C to stop.${NC}"
    echo ""
    
    watch_args=(--shell "CARGO_JOBS=${CARGO_JOBS} TEST_THREADS=${TEST_THREADS} $0 ${MODE:+--${MODE}} ${SPECIFIC_TEST:+--test $SPECIFIC_TEST}")
    cargo watch "${watch_args[@]}"
else
    run_tests
fi
