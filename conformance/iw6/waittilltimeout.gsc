// question: waittilltimeout(0.1, e) with no notify: when does it return?
// unsupported: IW6 has no waittilltimeout builtin: the compiler (gsc-tool) refuses the call
// expect compile error: couldn't determine function call type
// evidence: the compiler is gsc-tool (IW6 ships no compiler: its scripts are bytecode compiled offline)
// vs iw5: same

main()
{
	logprint("start");
	level waittilltimeout(0.1, "never");
	logprint("returned");
}
