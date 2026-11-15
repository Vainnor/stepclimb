# Third-party material in stepclimb-auth

## `src/password/common-passwords.txt`

The common-password list of `docs/specs/m0/04-local-accounts.md` Q8 (c): 10,000 lowercase
passwords of 12 to 256 characters, the length the policy allows, so every entry can match. It is
the 10 such entries of SecLists' `Passwords/Common-Credentials/10k-most-common.txt`, followed by
the most common other such entries of
`Passwords/Common-Credentials/xato-net-10-million-passwords-1000000.txt`, in that file's order,
each lowercased and kept once. Downloaded on 2026-11-15 from
<https://github.com/danielmiessler/SecLists> (`10k-most-common.txt` at commit `913b327`,
SHA-256 `68782d6a…0caf51`; the xato file at commit `c205c36`, SHA-256 `424a3e03…0fdcce`).

SecLists is distributed under the MIT License:

```text
MIT License

Copyright (c) 2018 Daniel Miessler

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
