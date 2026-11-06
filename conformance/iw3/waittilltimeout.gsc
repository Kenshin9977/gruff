// question: waittilltimeout(0.1, e) with no notify: when does it return?
// unsupported: IW3 has no waittilltimeout: the game's compiler refuses the call ('unknown function')
// expect compile error: unknown function
// vs t4: same (the form cannot exist in T4 either)

main()
{
	logprint("start");
	level waittilltimeout(0.1, "never");
	logprint("returned");
}
