.PHONY: help version-bump release build test clean

help:
	@echo "Shelltrax Makefile"
	@echo ""
	@echo "Usage:"
	@echo "  make version-bump VERSION=0.2.0    - Bump version in Cargo.toml, commit, and tag"
	@echo "  make release VERSION=0.2.0         - Bump version and push tag to trigger release"
	@echo "  make build                         - Build release binary"
	@echo "  make test                          - Run tests"
	@echo "  make clean                         - Clean build artifacts"
	@echo ""
	@echo "Examples:"
	@echo "  make version-bump VERSION=0.2.0    - Creates tag v0.2.0 locally"
	@echo "  make release VERSION=0.2.0         - Creates tag and pushes (triggers CI)"

# Bump version in Cargo.toml, commit, and create git tag
version-bump:
ifndef VERSION
	$(error VERSION is not set. Use: make version-bump VERSION=0.2.0)
endif
	@echo "Creating release branch for version $(VERSION)..."
	@git checkout -b release/v$(VERSION)
	@echo "Bumping version to $(VERSION)..."
	@sed -i 's/^version = .*/version = "$(VERSION)"/' Cargo.toml
	@git add Cargo.toml
	@git commit -m "chore: bump version to $(VERSION)"
	@git tag -a v$(VERSION) -m "Release v$(VERSION)"
	@echo ""
	@echo "✓ Created branch release/v$(VERSION)"
	@echo "✓ Version bumped to $(VERSION)"
	@echo "✓ Commit created"
	@echo "✓ Tag v$(VERSION) created"
	@echo ""
	@echo "To merge and push:"
	@echo "  git checkout main"
	@echo "  git moff release/v$(VERSION)"
	@echo "  git push origin main"
	@echo "  git push origin v$(VERSION)"
	@echo ""
	@echo "Or use: make release VERSION=$(VERSION)"

# Bump version and push tag to trigger GitHub Actions release
release: version-bump
	@echo "Merging into main..."
	@git checkout main
	@git moff release/v$(VERSION)
	@echo "Pushing to origin..."
	@git push origin main
	@git push origin v$(VERSION)
	@echo ""
	@echo "✓ Merged release/v$(VERSION) into main"
	@echo "✓ Pushed to main"
	@echo "✓ Pushed tag v$(VERSION)"
	@echo "✓ GitHub Actions will build release binaries"

# Build release binary
build:
	cargo build --release

# Run tests
test:
	cargo test

# Run clippy
clippy:
	cargo clippy

# Clean build artifacts
clean:
	cargo clean
