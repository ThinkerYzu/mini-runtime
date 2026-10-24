# Publication hygiene

This repository starts from a reviewed source snapshot with fresh Git history. It includes runtime code, build inputs, source-local documentation, tools and synthetic test fixtures. Historical benchmark reports and the development repository's Git history are not included.

## Data to keep local

Keep browser profiles, cookies, access tokens, login data, page captures, screenshots, network replays, console/request logs, environment recordings and measurement outputs out of public commits. Record your own benchmark inputs locally when opting into recorded-data checks. The `.gitignore` rules restrict generated files under `bench/`; ignore rules do not remove already tracked files or historical commits.

`tools/publication-files.txt` lists the initial approved files. Run `python3 tools/check-publication.py` before publishing changes. Add a new source or synthetic fixture to that manifest only after reviewing its contents. A file being small, synthetic-looking or ignored by a test does not establish that it contains no private data.

## Configurable inputs

- The default `tools/build.py` setup fetches public Mozilla source at the pinned revision into its cache. Optional SDK consumer builds select `MRT_SDK_DIR`; source builds explicitly select `MRT_ENGINE_SOURCE=1` and `MRT_JS_OBJDIR`. No Cargo manifest paths refer to Firefox. SDK artifacts remain separate from the source repository.
- Set `FIREFOX_BIN` for browser recording and comparison tools.
- Set `GDOC_URL` to your own document when selecting the optional Google Docs benchmark. There is no embedded personal document URL.
- Android experiments need an external harness selected with `MRT_PHONE_SUPPORT`; that harness is not included or needed for the standalone HTTP viewer.

## Checks before sharing

Review both the working files and the commits being published. The initial snapshot was checked with a credential scanner, personal-path/account/URL pattern checks, fixture metadata inspection and source review. These checks reduce risk but cannot guarantee the absence of every sensitive value. Third-party author names, email attribution and public specification links are retained where they belong to upstream code or license notices.

For a repeatable credential check, install Gitleaks from its official distribution and run:

```sh
gitleaks dir --redact=100 .
gitleaks git --redact=100 .
```

Do not commit scanner reports containing matched content. The initial commit uses a neutral contributor identity instead of importing private development commit metadata. No remote is configured by this snapshot preparation.

## Licensing

Existing third-party license and copyright notices are retained, including Firefox/Stylo notices, HarfBuzz's `COPYING` and the deterministic helper's license. This snapshot does not select or grant a new project-wide license for original code. The project owner must settle that choice and review the applicable third-party terms before release. Do not remove attribution as part of a privacy cleanup.

## Initial snapshot validation

The sanitized source built both release binaries with a compatible engine. Validation passed: 35 HTTP viewer tests (including 11 Firefox 157 browser cases), 14 standalone tests, four isolated recorded-data policy tests, and 26 API unit tests with one fixture-dependent test ignored by default. Temporary build outputs and test captures were removed from the publication directory afterwards. Credential scans used Gitleaks 8.30.1 with redacted reports kept outside this repository.

## SDK artifacts

SDK manifests record only public provenance, toolchain versions, target/ABI requirements and content hashes. Engine/header/license inventories are pinned independently of archive transport. Keep personal environment recordings and profiles out of SDKs and source commits. See [production and release gates](sdk-build-plan.md#release-checks). The current local SDK is not a public release: the existing producer engine includes source-path diagnostic strings. A controlled build with remapped paths and an updated reviewed pin is required before binary publication.
