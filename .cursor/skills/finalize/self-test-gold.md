# Parent only. Do not paste to a child.

| Id | Useful? |
| --- | --- |
| P1 | yes |
| P2 | no |
| P3 | yes |
| P4 | no |
| P5 | no |
| P6 | yes |
| P7 | no |
| P8 | no |
| P9 | no |
| P10 | no |

Pass: every id matches. Fail: any mismatch or a missing id.
P1 is a real wrap failure. P3 is `lookup::matching_store_version`.
P6 is `spec::replace_version`. The rest is taste, a new generic,
or a `wc -l` move.
