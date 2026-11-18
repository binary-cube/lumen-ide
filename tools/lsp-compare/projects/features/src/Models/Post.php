<?php

namespace App\Models;

class Post
{
    public function __construct(
        public string $title,
        public User $author,
    ) {
    }

    public function publish(?\DateTimeInterface $at = null): void
    {
    }
}
