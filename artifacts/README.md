# Artifact policy

Commit source, reproducible tests, lessons, and concise written results.
Generated output stays out of normal code reviews.

| Content | Location | Commit? |
|---|---|---|
| Compiled kernels, ELF files, disposable disk images | `target/` or the target's build directory | No |
| UART captures, test logs, screenshots, packet captures | `target/runs/<date>-<purpose>/` | No |
| Temporary diagnostic output | Operating-system temporary directory | No |
| Hardware result summary | Relevant `LEARNING.md` session entry | Yes |
| Durable hardware investigation, commands and checksums | A named Markdown document under `artifacts/` | Yes, when needed |
| Recovery SD images worth preserving | External image store recorded in `pi2-images.json` | Manifest only |

A result summary should identify the scenario, build/commit where available,
command or test, observed outcome, and any remaining limitation. Include a short
marker excerpt only when it explains the finding. Do not copy entire run logs
into the summary.

Raw outputs and compiled probes previously tracked under the baseline folders
were removed from the Git index during the November 2026 cleanup. Existing local
copies are retained and ignored. A fresh clone contains the written findings and
checksums; historical copies can be recovered from earlier commits. These files
are historical evidence, not guaranteed rebuildable releases.

Keep code/test changes and teaching/documentation changes in separate commits
when they are independently reviewable. Stage named paths and inspect the staged
file list before committing. Do not force-add generated output for routine test
runs. A deliberately maintained binary test fixture requires an explicit reason
and a dedicated fixture location.
