<?php

namespace App;

class Legacy
{
    /**
     * @deprecated Use modern() instead.
     */
    public function old(): void
    {
    }

    public function modern(): void
    {
    }
}

#[\Deprecated(message: 'Use format_name() instead')]
function old_format(): string
{
    return '';
}
