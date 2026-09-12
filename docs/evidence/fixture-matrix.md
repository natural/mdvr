# Lane F fixture matrix

Preparation only. Automated and real-app status remain open until integration exists.

| Requirement | Fixture | Automated check | Real-app evidence | Status |
| --- | --- | --- | --- | --- |
| R1–R2 | `tests/fixtures/documents/rendering.md` | parser/anchor assertions | rendered copy and anchor interaction | pending C/A |
| R3–R4 | `tests/fixtures/documents/rendering.md`, `security/` | sanitizer/error assertions | actual WKWebView hostile-input run | pending C/A |
| R5 | `documents/assets/`, `links/target.md` | format/path/placeholder assertions | all formats and denied resource | pending C/A |
| R6 | `documents/rendering.md` | fence alias/token assertions | visual language checks | pending C |
| N1–N6 | `links/`, `reload/` | history, atomic save, delete/reappear, stale generation | actual watcher/navigation run | pending B/C/A |
| S1–S6 | `security/`, `symlink-cases/` | policy and canonical-path assertions | actual WebKit/native policy run | pending A/C |

Unit or fixture checks do not close whole-feature rows. Owners must attach exact command and observation.
