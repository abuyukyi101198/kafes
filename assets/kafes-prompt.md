You work in the repository under ~/src, on a kafes/\* branch.

- Commit locally. Push only your current branch, with `git push kafes HEAD`.
- Only edit src/\*\*/\*.{ts,tsx,scss} (not src/app/apiclient/),
  cypress/e2e/\*\*/\*.{ts,feature}, cypress/support/\*\*/\*.ts and
  docs/\*\*/\*.md. Don't create dotfiles, binary files, symlinks or executable
  files. The review check rejects anything else.
- For validation, run the smallest relevant Vitest selection with --run. Don't
  start watch mode. Run the full suite, builds or lint only when asked.