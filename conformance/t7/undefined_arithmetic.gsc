// question: what does undefined + 1 do at run time: an error that ends the thread, or a value?
// fact: undefined + 1 is a non-terminal script error: the thread goes on and the result is undefined
// expect: f0 T0 before
// expect: f0 T0 script error: pair 'undefined' and '1' has unmatching types 'undefined' and 'int'
// expect: f0 T0 after 0
#namespace zz_test;

function main()
{
	logprint("before");
	x = undefined;
	y = x + 1;
	logprint("after " + isdefined(y));
}
