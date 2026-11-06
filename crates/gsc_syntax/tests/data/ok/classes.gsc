class demo_base
{
	var m_count;
	var m_name = "base";

	constructor()
	{
		m_count = 0;
	}

	destructor()
	{
	}

	function bump()
	{
		m_count++;
	}
}

class demo_child : demo_base
{
	function describe()
	{
		return m_name + m_count;
	}
}
