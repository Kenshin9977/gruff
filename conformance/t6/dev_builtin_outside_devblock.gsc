// question: a dev-only builtin (println) called outside /# #/: what does the game's linker do at load?
// fact: a dev-only builtin (println) outside /# #/: the game's linker refuses the script: 'Dev only calls must be wrapped in a devblock /#...#/' (Com_Error at load)
// vs t7: same
// evidence: static: the linker (0x7e098f) checks the developer flag the builtin lookup returns against the import's developer bit 0x10
// expect error: Dev only calls must be wrapped in a devblock

main()
{
	println("x");
}
