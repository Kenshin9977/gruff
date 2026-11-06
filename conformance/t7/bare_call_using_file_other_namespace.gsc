// question: a bare call to a function that only a #using'd file of another namespace defines: does the game's linker find it?
// with: zz_bare_lib.gsc
// fact: a bare call never reaches another namespace, even of a #using'd file: the game's linker reports an unresolved external at load
// expect error: Unresolved external "4308497F"
// claim: bo3-gsc-language-doc (agrees): an unqualified call resolves in the current namespace first, then falls back to sys
// evidence: compile side: the official linker emits an import of zz_test::lib_only (evidence/linker-t7/bare_call_other_file.json); load side: the game's linker reports it unresolved
#using scripts\zz_harness\zz_bare_lib;
#namespace zz_test;

function main()
{
	a = lib_only();
	logprint("lib_only=" + a);
}
