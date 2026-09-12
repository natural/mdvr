# Hostile HTML and SVG

<script>alert('document script')</script>
<div onclick="alert(1)" style="background:url(https://remote.invalid/x)">event and CSS</div>
<iframe src="https://remote.invalid/frame"></iframe>
<form action="file:///etc/passwd"><input name="x"></form>
<object data="file:///etc/passwd"></object>

![hostile svg](hostile.svg)

[script scheme](javascript:alert(1))
[unknown scheme](gopher://127.0.0.1/)
