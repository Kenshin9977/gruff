// question: do two hashes of names differing only in case compare equal, and does a string equal the hash of itself?
// unsupported: IW5 has no hash literals (#"..."): the compiler (gsc-tool) refuses '#'
// expect compile error: unexpected token '#'
// evidence: the compiler is gsc-tool (IW5 ships no compiler: its scripts are bytecode compiled offline)
// vs iw3: same (the form cannot exist in IW3 either)

main()
{
	a = #"GoAway";
	b = #"goaway";
	logprint("hash case=" + (a == b));
	c = "goaway";
	logprint("string vs hash=" + (c == b));
}
