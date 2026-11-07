// question: do two hashes of names differing only in case compare equal, and does a string equal the hash of itself?
// unsupported: H1 has no hash literals (#"..."): the compiler (gsc-tool) refuses '#'
// expect compile error: unexpected token '#'
// evidence: the compiler is gsc-tool (H1 ships no compiler: its scripts are bytecode compiled offline)
// vs s1: same

main()
{
	a = #"GoAway";
	b = #"goaway";
	logprint("hash case=" + (a == b));
	c = "goaway";
	logprint("string vs hash=" + (c == b));
}
