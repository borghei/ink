# Hostile document

Title attack: ]0;pwned-title

Clear screen: [2J[H

Text with raw escape: [31mred[0m and OSC: ]52;c;bWFsaWNpb3Vz

```
code block with escape: ]0;pwned-from-code
cursor moves: [10A[5B
```

Inline `code ]8;;http://evil.example\click\x1b]8;;` too.

[Innocent looking link](https://example.com/]8;;]0;pwned)

[File link](file:///etc/passwd)

[JS link](javascript:alert(1))

[Data link](data:text/html,<script>x</script>)

[Good link](https://example.com/ok)

![escape image](../../../../