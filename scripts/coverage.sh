#!/bin/bash
# Code coverage script for Saorsa Gossip

set -e

echo "🔍 Running code coverage analysis..."

# Install cargo-tarpaulin if not present
if ! command -v cargo-tarpaulin &> /dev/null; then
    echo "📦 Installing cargo-tarpaulin..."
    cargo install cargo-tarpaulin
fi

# Clean previous coverage data
echo "🧹 Cleaning previous coverage data..."
cargo clean

# Run coverage with tarpaulin
echo "📊 Running coverage analysis..."
cargo tarpaulin \
    --workspace \
    --all-features \
    --timeout 300 \
    --out xml \
    --out html \
    --output-dir target/coverage \
    --skip-clean

# Generate coverage report
echo "📈 Generating coverage report..."
if [ -f "target/coverage/tarpaulin-report.html" ]; then
    echo "✅ HTML report generated: target/coverage/tarpaulin-report.html"
    echo "🌐 Open the report in your browser to view detailed coverage"
fi

if [ -f "target/coverage/cobertura.xml" ]; then
    echo "✅ XML report generated: target/coverage/cobertura.xml"
    echo "📊 This can be used with CI systems like Codecov"
fi

# Show coverage summary
echo ""
echo "📋 Coverage Summary:"
cargo tarpaulin \
    --workspace \
    --all-features \
    --skip-clean \
    --count \
    --ignore-tests

echo ""
echo "✨ Coverage analysis complete!"