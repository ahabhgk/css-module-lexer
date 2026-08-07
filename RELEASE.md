# Release

1. Create a release branch from the latest `main`:

   ```console
   VERSION=0.0.1 # Replace it with the actual version
   git switch main
   git pull --ff-only
   git switch -c "release/v$VERSION"
   ```

2. Manually update `package.version` in `Cargo.toml`, then update and verify
   `Cargo.lock`:

   ```console
   cargo check
   git diff -- Cargo.toml Cargo.lock
   ```

3. Run checks, commit, and push the release branch:

   ```console
   cargo fmt --check
   cargo clippy --locked -- -D warnings
   cargo test --locked
   git add Cargo.toml Cargo.lock
   git commit -m "chore: release v$VERSION"
   git push -u origin "release/v$VERSION"
   ```

4. Create the release pull request:

   ```console
   gh pr create --base main --head "release/v$VERSION" \
     --title "chore: release v$VERSION" \
     --body "Release v$VERSION."
   ```

5. After CI passes, manually run the release workflow on the release branch:

   ```console
   gh workflow run release.yml --ref "release/v$VERSION"
   gh run watch
   ```

6. Wait for the workflow to publish the crate and push the `v$VERSION` tag.

7. Merge the pull request into `main`, then update the local branch:

   ```console
   gh pr merge "release/v$VERSION"
   git switch main
   git pull --ff-only
   ```

8. Generate release notes and publish the GitHub release:

   ```console
   gh release create "v$VERSION" --verify-tag \
     --title "v$VERSION" --generate-notes
   ```
