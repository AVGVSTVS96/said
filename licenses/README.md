# License sources

[NOTICE](../NOTICE) is the attribution index. Engine/runtime terms are in `../crates/engine/licenses`, adapted macOS terms in `../crates/platform/macos/THIRD-PARTY.md`, and corpus provenance in `../bench/LICENSES.md`. Font OFLs and Creative Commons terms are retained here.

`upstream.json` maps crates whose published archive omits license files to the corresponding upstream texts. GitHub URLs use the revision from the crate's `.cargo_vcs_info.json`; the file's SHA-256 is checked before packaging. `selectors` uses the official MPL 2.0 text, matching its source headers. `realfft` declares MIT in its published metadata/README without a separate license file; its notice retains that author attribution and the SPDX MIT permission terms without inventing a copyright statement. The generated dependency notice includes versioned crate source links for MPL-covered code.

After installing locked frontend dependencies, generate the full Rust/JavaScript inventory:

```sh
bun packaging/notices.ts /path/outside/repo/dependencies.txt
```

The inventory covers both supported host targets, including their build/test graphs, and follows frontend production dependencies. It reads complete license/copyright/NOTICE files from package sources and fails on missing terms. Packaging regenerates it for each artifact, so a lockfile change is reflected in the distributed text. New crates that omit terms from their archive need a verified, hashed upstream entry.

This retains upstream licensing as supplied. It does not relicense those components or assert that every inventoried build/test dependency is in the binary.
