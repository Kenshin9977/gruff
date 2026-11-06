// External scanner for tree-sitter-gsc: the tokens a regular grammar cannot express.
// SPDX-License-Identifier: MIT
//
// - preproc_disabled: from `#else`/`#elif` to the `#endif` closing its group (nesting aware),
//   as crates/gsc_syntax/src/lexer.rs does (the first branch of a group is the active one).
// - encoded_name: an animation name acts printed as escaped bytes (`%\x83\x99...`); its third
//   decoded byte is the length of the rest.
// - _macro_name: an upper-case macro used as a statement with no `;` (`WAIT_SERVER_FRAME`,
//   `MACRO( x )` alone on its line), or gsc-tool's `__asm_jmp( loc )`.

#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <stdint.h>

enum TokenType {
  PREPROC_DISABLED,
  ENCODED_NAME,
  MACRO_NAME,
  DECOMPILED_PATH,
  DECOMPILED_NAME,
  DIRECTIVE_END,
  ERROR_SENTINEL,
};

void *tree_sitter_gsc_external_scanner_create(void) { return NULL; }
void tree_sitter_gsc_external_scanner_destroy(void *payload) { (void)payload; }
unsigned tree_sitter_gsc_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}
void tree_sitter_gsc_external_scanner_deserialize(void *payload, const char *buffer,
                                                  unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }
static inline void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

static inline bool is_word(int32_t c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_';
}

static inline bool is_hspace(int32_t c) { return c == ' ' || c == '\t' || c == '\v' || c == '\f'; }

static inline bool is_newline(int32_t c) { return c == '\n' || c == '\r'; }

static inline bool is_hex(int32_t c) {
  return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
}

static int hex_value(int32_t c) {
  if (c >= '0' && c <= '9') return c - '0';
  if (c >= 'a' && c <= 'f') return c - 'a' + 10;
  return c - 'A' + 10;
}

// Read a word into `buf` (truncated to `cap - 1` bytes, NUL-terminated); returns its length.
static unsigned read_word(TSLexer *lexer, char *buf, unsigned cap) {
  unsigned n = 0;
  while (is_word(lexer->lookahead) && !lexer->eof(lexer)) {
    if (n + 1 < cap) buf[n] = (char)lexer->lookahead;
    n++;
    advance(lexer);
  }
  buf[n + 1 < cap ? n : cap - 1] = 0;
  return n;
}

static bool str_eq(const char *a, const char *b) {
  while (*a && *a == *b) {
    a++;
    b++;
  }
  return *a == *b;
}

// Skip to the start of the next line.
static void skip_line(TSLexer *lexer) {
  while (!lexer->eof(lexer) && !is_newline(lexer->lookahead)) advance(lexer);
  while (!lexer->eof(lexer) && is_newline(lexer->lookahead)) advance(lexer);
}

// After `#else` or `#elif`: consume every line up to the `#endif` closing this group.
static bool scan_disabled(TSLexer *lexer) {
  unsigned depth = 0;
  skip_line(lexer);
  lexer->mark_end(lexer);
  while (!lexer->eof(lexer)) {
    while (is_hspace(lexer->lookahead)) advance(lexer);
    if (lexer->lookahead == '#') {
      advance(lexer);
      char word[8];
      read_word(lexer, word, sizeof word);
      if (str_eq(word, "if") || str_eq(word, "ifdef") || str_eq(word, "ifndef")) {
        depth++;
      } else if (str_eq(word, "endif")) {
        if (depth == 0) return true;
        depth--;
      }
    }
    skip_line(lexer);
    lexer->mark_end(lexer);
  }
  lexer->mark_end(lexer);
  return true;
}

static bool scan_encoded_name(TSLexer *lexer) {
  if (lexer->lookahead != '\\') return false;
  unsigned decoded = 0;
  unsigned total = UINT32_MAX;
  bool first = true;
  while (!lexer->eof(lexer) && !is_newline(lexer->lookahead) && decoded < total) {
    int32_t c = lexer->lookahead;
    unsigned byte;
    bool blank = false;
    if (c == '\\') {
      advance(lexer);
      int32_t e = lexer->lookahead;
      if (e == 'x') {
        advance(lexer);
        if (!is_hex(lexer->lookahead)) {
          if (first) return false;
          byte = 'x';
        } else {
          byte = (unsigned)hex_value(lexer->lookahead);
          advance(lexer);
          if (is_hex(lexer->lookahead)) {
            byte = byte * 16 + (unsigned)hex_value(lexer->lookahead);
            advance(lexer);
          }
        }
      } else if (lexer->eof(lexer) || is_newline(e)) {
        if (first) return false;
        byte = '\\';
      } else {
        if (first) return false;
        advance(lexer);
        switch (e) {
          case 'a': byte = 7; break;
          case 'b': byte = 8; break;
          case 't': byte = 9; break;
          case 'n': byte = 10; break;
          case 'v': byte = 11; break;
          case 'f': byte = 12; break;
          case 'r': byte = 13; break;
          default: byte = (unsigned)e & 0xFF; break;
        }
      }
    } else {
      blank = is_hspace(c);
      byte = (unsigned)c & 0xFF;
      advance(lexer);
    }
    first = false;
    decoded++;
    if (decoded == 3) total = byte + 2;
    // Without a usable header the name stops before trailing blanks.
    if (!blank || decoded >= total) lexer->mark_end(lexer);
  }
  lexer->result_symbol = ENCODED_NAME;
  return true;
}

// Skip blanks, line breaks and comments; returns whether a line break was crossed.
static bool skip_trivia(TSLexer *lexer, bool *ok) {
  bool newline = false;
  for (;;) {
    int32_t c = lexer->lookahead;
    if (lexer->eof(lexer)) return newline;
    if (is_newline(c)) {
      newline = true;
      advance(lexer);
    } else if (is_hspace(c) || c == 0xA0 || c == 0xFEFF) {
      advance(lexer);
    } else if (c == '/') {
      advance(lexer);
      if (lexer->lookahead == '/') {
        while (!lexer->eof(lexer) && !is_newline(lexer->lookahead)) advance(lexer);
      } else if (lexer->lookahead == '*') {
        advance(lexer);
        int32_t prev = 0;
        while (!lexer->eof(lexer) && !(prev == '*' && lexer->lookahead == '/')) {
          if (is_newline(lexer->lookahead)) newline = true;
          prev = lexer->lookahead;
          advance(lexer);
        }
        advance(lexer);
      } else {
        // `/#` opens a dev block (a new statement); any other `/` is division.
        *ok = lexer->lookahead == '#';
        return newline;
      }
    } else {
      return newline;
    }
  }
}

// Skip a balanced `( ... )`, strings included. The lexer is on the `(`.
static bool skip_parens(TSLexer *lexer) {
  unsigned depth = 0;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (c == '"') {
      advance(lexer);
      while (!lexer->eof(lexer) && lexer->lookahead != '"' && !is_newline(lexer->lookahead)) {
        if (lexer->lookahead == '\\') advance(lexer);
        advance(lexer);
      }
      if (lexer->lookahead != '"') return false;
      advance(lexer);
      continue;
    }
    if (c == '(') depth++;
    if (c == ')') {
      depth--;
      if (depth == 0) {
        advance(lexer);
        return true;
      }
    }
    if (c == ';' || c == '{' || c == '}') return false;
    advance(lexer);
  }
  return false;
}

// A decompiler name glued with `/` (`dlc5/zmhd/fx_name`, `scripts/mp/x.gsc`). The lexer is past
// the first word, on a `/`. In a declaration (`declaration`: a function or parameter name) one `/`
// is enough. In an expression a division must stay one (`a/f( b )` in source): the name needs a
// `::` after it, or two `/` and a `(`.
static bool scan_decompiled_path(TSLexer *lexer, bool declaration) {
  unsigned slashes = 0;
  while (lexer->lookahead == '/') {
    advance(lexer);
    if (!is_word(lexer->lookahead)) return false;
    while (is_word(lexer->lookahead)) advance(lexer);
    slashes++;
  }
  while (lexer->lookahead == '.') {
    advance(lexer);
    if (!is_word(lexer->lookahead)) return false;
    while (is_word(lexer->lookahead)) advance(lexer);
  }
  lexer->mark_end(lexer);
  if (declaration) {
    lexer->result_symbol = DECOMPILED_NAME;
    return true;
  }
  while (is_hspace(lexer->lookahead)) advance(lexer);
  if (lexer->lookahead == '(') {
    lexer->result_symbol = DECOMPILED_PATH;
    return slashes >= 2;
  }
  if (lexer->lookahead != ':') return false;
  advance(lexer);
  if (lexer->lookahead != ':') return false;
  lexer->result_symbol = DECOMPILED_PATH;
  return true;
}

static bool scan_word_token(TSLexer *lexer, const bool *valid_symbols) {
  int32_t c = lexer->lookahead;
  if (!is_word(c)) return false;
  char word[8];
  bool upper = false;
  bool lower = false;
  unsigned n = 0;
  while (is_word(lexer->lookahead)) {
    c = lexer->lookahead;
    if (n < sizeof word - 1) word[n] = (char)c;
    n++;
    if (c >= 'A' && c <= 'Z') upper = true;
    if (c >= 'a' && c <= 'z') lower = true;
    advance(lexer);
  }
  word[n < sizeof word ? n : sizeof word - 1] = 0;
  if (lexer->lookahead == '/') {
    if (valid_symbols[DECOMPILED_NAME]) return scan_decompiled_path(lexer, true);
    return valid_symbols[DECOMPILED_PATH] && scan_decompiled_path(lexer, false);
  }
  if (!valid_symbols[MACRO_NAME] || (word[0] >= '0' && word[0] <= '9')) return false;
  bool asm_jump = n >= 5 && word[0] == '_' && word[1] == '_' && word[2] == 'a' &&
                  word[3] == 's' && word[4] == 'm';
  if (!asm_jump && (!upper || lower)) return false;
  lexer->mark_end(lexer);
  bool ok = true;
  bool newline = skip_trivia(lexer, &ok);
  if (!ok) return false;
  if (lexer->lookahead == '(') {
    if (!skip_parens(lexer)) return false;
    newline = skip_trivia(lexer, &ok);
    if (!ok) return false;
  }
  if (!newline && !lexer->eof(lexer)) return false;
  switch (lexer->lookahead) {
    case ';': case '.': case '=': case '+': case '-': case '*': case '%': case '<': case '>':
    case '&': case '|': case '^': case '?': case ':': case ',': case ')': case ']':
      return false;
    case '[':
      // `MACRO\n[[ f ]]()` is a new statement; `MACRO\n[ i ]` indexes it.
      advance(lexer);
      while (is_hspace(lexer->lookahead)) advance(lexer);
      if (lexer->lookahead != '[') return false;
      break;
    case '!':
      advance(lexer);
      if (lexer->lookahead == '=') return false;
      break;
    default:
      break;
  }
  lexer->result_symbol = MACRO_NAME;
  return true;
}

bool tree_sitter_gsc_external_scanner_scan(void *payload, TSLexer *lexer,
                                           const bool *valid_symbols) {
  (void)payload;
  bool recovering = valid_symbols[ERROR_SENTINEL];

  if (valid_symbols[ENCODED_NAME] && !recovering && lexer->lookahead == '\\') {
    return scan_encoded_name(lexer);
  }

  // The end of a `#define` line: its line break, or the end of the file.
  if (valid_symbols[DIRECTIVE_END] && !recovering) {
    while (is_hspace(lexer->lookahead)) skip(lexer);
    if (lexer->eof(lexer)) {
      lexer->result_symbol = DIRECTIVE_END;
      return true;
    }
    if (is_newline(lexer->lookahead)) {
      if (lexer->lookahead == '\r') advance(lexer);
      if (lexer->lookahead == '\n') advance(lexer);
      lexer->result_symbol = DIRECTIVE_END;
      return true;
    }
  }

  if (valid_symbols[PREPROC_DISABLED] || valid_symbols[MACRO_NAME] ||
      valid_symbols[DECOMPILED_PATH] || valid_symbols[DECOMPILED_NAME]) {
    bool line_start = lexer->get_column(lexer) == 0;
    while (is_hspace(lexer->lookahead) || is_newline(lexer->lookahead) ||
           lexer->lookahead == 0xFEFF || lexer->lookahead == 0xA0) {
      if (is_newline(lexer->lookahead)) line_start = true;
      skip(lexer);
    }
    if (lexer->lookahead == '#') {
      if (!valid_symbols[PREPROC_DISABLED] || !line_start) return false;
      advance(lexer);
      char word[8];
      read_word(lexer, word, sizeof word);
      if (!str_eq(word, "else") && !str_eq(word, "elif")) return false;
      lexer->result_symbol = PREPROC_DISABLED;
      return scan_disabled(lexer);
    }
    if (recovering || !(valid_symbols[MACRO_NAME] || valid_symbols[DECOMPILED_PATH] ||
                        valid_symbols[DECOMPILED_NAME])) {
      return false;
    }
    return scan_word_token(lexer, valid_symbols);
  }
  return false;
}
