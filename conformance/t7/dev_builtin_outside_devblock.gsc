// question: a dev-only builtin (println) called outside /# #/: what does the game's linker do at load?
// fact: the compiler accepts it, but the game's linker refuses to load the script: 'Dev only calls must be wrapped in a devblock /#...#/' (retail and developer mode alike)
// expect error: Dev only calls must be wrapped in a devblock
// evidence: the linker finds println's entry in the builtin tables (type 1, dev) and reports it; without the builtin tables the same import is an 'Unresolved external' (a harness check, not a game mode)
#namespace zz_test;

function main()
{
	println("x");
}
