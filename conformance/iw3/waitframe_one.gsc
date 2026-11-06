// question: how many server frames do waitframe(1) and waitframe(2) take, against wait 0.05?
// unsupported: IW3 has no waitframe: the game's compiler refuses the call ('unknown function')
// expect compile error: unknown function
// vs t4: same (the form cannot exist in T4 either)

main()
{
	logprint("start");
	waitframe(1);
	logprint("after waitframe 1");
	waitframe(2);
	logprint("after waitframe 2");
	wait 0.05;
	logprint("after wait 0.05");
}
