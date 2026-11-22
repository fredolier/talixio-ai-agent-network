#!/bin/sh
# Pose sur `main` l'état de la branche de travail, sans les documents internes.
# Usage : scripts/publier-main.sh "message du commit"   (puis : git push origin main)
# Ce script et les documents internes ne vont jamais sur `main`.
set -eu
[ $# -eq 1 ] || { echo "usage : $0 \"message\"" >&2; exit 2; }
PRIVE="docs/SPEC.md docs/PLAN.md docs/EXIGENCES.md docs/VERIFICATION.md CLAUDE.md scripts/publier-main.sh"
[ -z "$(git status --porcelain)" ] || { echo "arbre de travail non propre" >&2; exit 1; }
index="$(mktemp)"; trap 'rm -f "$index"' EXIT
GIT_INDEX_FILE="$index" git read-tree HEAD
# shellcheck disable=SC2086
GIT_INDEX_FILE="$index" git rm -q --cached --ignore-unmatch $PRIVE
tree="$(GIT_INDEX_FILE="$index" git write-tree)"
if parent="$(git rev-parse -q --verify refs/heads/main)"; then
  [ "$(git rev-parse "$parent^{tree}")" != "$tree" ] || { echo "main est déjà à jour"; exit 0; }
  commit="$(git commit-tree "$tree" -p "$parent" -m "$1")"
else
  commit="$(git commit-tree "$tree" -m "$1")"
fi
git update-ref refs/heads/main "$commit"
echo "main -> $commit (à pousser : git push origin main)"
