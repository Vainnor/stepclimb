# Third-party material in stepclimb-auth

## `src/password/common-passwords.txt`

The common-password list of `docs/specs/m0/04-local-accounts.md` Q8 (c): 10,000 lowercase
passwords of 12 to 256 characters, the lengths the policy allows, so every entry can match. It is
built from two SecLists files, downloaded on 2026-11-15 from
<https://github.com/danielmiessler/SecLists>:

- `Passwords/Common-Credentials/10k-most-common.txt` (commit `913b327`, SHA-256
  `68782d6a…0caf51`): its 10 entries of 12 or more characters, first and unfiltered, so the
  server refuses at least what the literal Q8 list would.
- `Passwords/Common-Credentials/xato-net-10-million-passwords-1000000.txt` (commit `c205c36`,
  SHA-256 `424a3e03…0fdcce`): then its entries of 12 to 256 characters in that file's order,
  each lowercased and kept once, until the list holds 10,000. This list makes no claim about how
  common each entry is beyond that file's order.

The xato entries pass these filters first, which drop anything that looks like an email or a
personal identifier, and offensive entries a simple substring check finds:

- an `@`, a web or mail domain (`.com`, `.net` or `.org` anywhere, `.ru` and other common
  endings, `www.`, `http:`, `mail`);
- a run of 10 or more digits that is not an ascending or descending sequence or a repetition of
  a unit of up to 6 digits (phone-number-like);
- letters followed by 6 or 8 digits that read as a calendar date (a name and a birth date);
- one of 31 offensive substrings (profanity, slurs and sexual terms).

Of the entries considered, the filters dropped 763 as personal-looking and 202 as offensive. A
substring filter cannot catch every name or slur: the list still holds first names followed by a
year and other entries a person may find offensive. Five of the 10 entries from the first file
are offensive words; they stay so that file is covered in full.

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
