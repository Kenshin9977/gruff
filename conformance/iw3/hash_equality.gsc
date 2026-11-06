// question: do two hashes of names differing only in case compare equal, and does a string equal the hash of itself?
// unsupported: IW3 has no hash literals (#"..."): the game's lexer refuses '#' (bad token)
// expect compile error: bad token '#'
// vs t4: same (the form cannot exist in T4 either)

main()
{
	a = #"GoAway";
	b = #"goaway";
	logprint("hash case=" + (a == b));
	c = "goaway";
	logprint("string vs hash=" + (c == b));
}
