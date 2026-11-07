// question: waittilltimeout(0.1, e) with no notify: when does it return?
// unsupported: IW5 has no waittilltimeout builtin: the compiler (gsc-tool) refuses the call
// expect compile error: couldn't determine function call type
// evidence: the compiler is gsc-tool (IW5 ships no compiler: its scripts are bytecode compiled offline)
// vs iw3: same (the form cannot exist in IW3 either)

main()
{
	logprint("start");
	level waittilltimeout(0.1, "never");
	logprint("returned");
}
