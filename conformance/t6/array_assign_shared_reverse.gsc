// question: after b = a, is a write through a seen through b (the other direction)?
// fact: after b = a, a write through a is not seen through b: a shared array is copied when written (copy on write), so b keeps the old empty array
// vs t7: assignment shares the array both ways (b.size=1 b[0]=7)
// evidence: static: the array-reference path 0x7e9640 (used for every write through an array): when the array object's reference count (u16 at +0 of its object entry) is not 0, RemoveRefToObject (0x7e7cb0), Scr_AllocArray (0x7e58d0) and CopyArray (0x7e5e30) give the variable its own copy (0x7e9882-0x7e98a4): copy on write
// expect: f0 T0 script error: pair 'b.size=0 b[0]=' and 'undefined' has unmatching types 'string' and 'undefined'
// expect: f0 T0 undefined

main()
{
	a = [];
	b = a;
	a[0] = 7;
	logprint("b.size=" + b.size + " b[0]=" + b[0]);
}
