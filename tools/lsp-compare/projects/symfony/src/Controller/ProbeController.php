<?php

namespace App\Controller;

use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;
use Symfony\Component\HttpFoundation\Request;
use Symfony\Component\HttpFoundation\Response;
use Symfony\Component\Routing\Attribute\Route;

/**
 * The probes' code in a Symfony project: each `// @Cnn` line is replaced by a probe's text
 * while it's asked; the rest is asked where it stands.
 */
final class ProbeController extends AbstractController
{
    #[Route('/probe', name: 'probe')]
    public function index(Request $request): Response
    {
        // @C36
        // @C37
        // @C38
        // @C39
        // @C40
        // @C41
        // @C42
        $page = $request->query->getInt('page', 1);
        if ($page > 1) {
            return $this->redirectToRoute('probe');
        }
        return new Response('Page ' . $page);
    }

    #[Route('/probe/view', name: 'probe_view')]
    public function view(): Response
    {
        return $this->render('base.html.twig', ['title' => 'Probe']);
    }
}
