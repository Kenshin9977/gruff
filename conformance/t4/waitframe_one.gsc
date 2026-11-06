// question: how many server frames do waitframe(1) and waitframe(2) take, against wait 0.05?
// unsupported: T4 has no waitframe: the game's compiler refuses the call ('unknown function')
// expect compile error: unknown function
// vs t7: T7 has no waitframe either (T8: n frames)

main()
{
	iprintln("start");
	waitframe(1);
	iprintln("after waitframe 1");
	waitframe(2);
	iprintln("after waitframe 2");
	wait 0.05;
	iprintln("after wait 0.05");
}
