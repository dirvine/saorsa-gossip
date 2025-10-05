#!/bin/bash
# Medium priority improvements script for Saorsa Gossip

set -e

echo "🚀 Running medium priority improvements..."

# 1. Run integration tests
echo ""
echo "📋 1. Running integration tests..."
echo "================================="
if cargo test --test integration_tests --workspace --all-features; then
    echo "✅ Integration tests passed"
else
    echo "❌ Integration tests failed"
    exit 1
fi

# 2. Run performance benchmarks
echo ""
echo "⚡ 2. Running performance benchmarks..."
echo "======================================"
if cargo bench --bench performance --workspace --all-features; then
    echo "✅ Benchmarks completed successfully"
    echo "📊 Check target/criterion/report/index.html for detailed results"
else
    echo "❌ Benchmarks failed"
    exit 1
fi

# 3. Generate code coverage report
echo ""
echo "📊 3. Generating code coverage report..."
echo "======================================="
if ./scripts/coverage.sh; then
    echo "✅ Coverage report generated"
    echo "📈 Check target/coverage/tarpaulin-report.html for detailed coverage"
else
    echo "❌ Coverage generation failed"
    exit 1
fi

# 4. Verify all tests still pass
echo ""
echo "🧪 4. Verifying all tests pass..."
echo "==============================="
if cargo test --workspace --all-features; then
    echo "✅ All tests pass"
else
    echo "❌ Some tests failed"
    exit 1
fi

# 5. Check code quality
echo ""
echo "🔍 5. Checking code quality..."
echo "============================"
if cargo clippy --workspace --all-features --all-targets -- -D warnings; then
    echo "✅ No clippy warnings"
else
    echo "❌ Clippy warnings found"
    exit 1
fi

if cargo fmt --all -- --check; then
    echo "✅ Code properly formatted"
else
    echo "❌ Code formatting issues"
    exit 1
fi

echo ""
echo "✨ All medium priority improvements completed successfully!"
echo ""
echo "📋 Summary:"
echo "  - ✅ Integration tests added and passing"
echo "  - ✅ Performance benchmarks implemented"
echo "  - ✅ Code coverage reporting set up"
echo "  - ✅ Transport layer TODO items completed"
echo ""
echo "📊 Generated reports:"
echo "  - Performance: target/criterion/report/index.html"
echo "  - Coverage: target/coverage/tarpaulin-report.html"