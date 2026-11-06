// question: do two hashes of names differing only in case compare equal, and does a string equal the hash of itself?
// unsupported: T4 has no hash literals (#"..."): the game's lexer refuses '#' (bad token)
// expect compile error: bad token '#'
// vs t7: T7: == between hashes, or a string and a hash, is a script error that yields false

main()
{
	a = #"GoAway";
	b = #"goaway";
	iprintln("hash case=" + (a == b));
	c = "goaway";
	iprintln("string vs hash=" + (c == b));
}
