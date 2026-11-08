# Wide tables

A short key column next to a long description:

| Key | Description |
|---|---|
| timeout | The number of milliseconds to wait before the request is aborted. Defaults to 30000. Set to 0 to disable entirely. |
| retries | How many times to retry. |

Numbers that must not be split:

| Item | Region | Q1 | Q2 | Total |
|---|---|--:|--:|--:|
| Alpha Widgets, the long-running flagship product line | EMEA | 1,204.50 | 998.00 | 2,202.50 |
| Beta | APAC | -12.00 | $3,000 | 15% |

Many columns:

| ID | Name | Region | Q1 | Q2 | Q3 | Q4 | Total | Status | Owner |
|---|---|---|--:|--:|--:|--:|--:|---|---|
| 1 | Alpha Widgets | EMEA | 1,204.50 | 998.00 | 1,450.25 | 2,001.00 | 5,653.75 | active | jsmith |

URLs, inline code and wide characters:

| Service | Endpoint | Notes |
|---|---|---|
| auth | `https://auth.internal.example.com/v2/oauth/token?grant_type=client_credentials` | [docs](https://example.com/docs) |
| 日本語サービス | `GET /api/v1/ユーザー/{id}/設定` | 👨‍👩‍👧‍👦 family ⚠️ warn |

A header that wraps:

| Name<br>(full) | Value |
|---|---|
| x | y |

- In a list:

  | a | b |
  |---|---|
  | one two three four five six seven eight nine ten | eleven |
