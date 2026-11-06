// question: waittilltimeout(0.1, e) with no notify: when does it return?
// unsupported: T4 has no waittilltimeout: the game's compiler refuses the call ('unknown function')
// expect compile error: unknown function
// vs t7: T7 has no waittilltimeout either (T8: after 3 frames)

main()
{
	iprintln("start");
	level waittilltimeout(0.1, "never");
	iprintln("returned");
}
