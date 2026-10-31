//! Prints the built-in color themes as a TypeScript module, for `web/src/themes.ts`.
//! Used by `pnpm gen:themes` in web/; a test in the contract crate checks
//! that the checked-in file matches.

fn main() {
    print!("{}", dashboard_contract::theme::builtins_ts());
}
