<?php

namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\HttpFoundation\Response;

use function Symfony\Component\Translation\t;

/**
 * Names Symfony doesn't know, for the probes (plan 65d): a route, a service, a translation.
 */
final class SymfonyProblems extends AbstractController
{
    public function missing(): Response
    {
        $this->container->get('no.such.service');
        $label = t('probe.missing');
        return $this->redirectToRoute('nowhere');
    }
}
