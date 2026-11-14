from django.db import models
from django.test import TestCase
class Post(models.Model):
    title = models.CharField(max_length=10)
    class Meta:
        ordering = ["title"]
    def __str__(self):
        return self.title
    def save(self, *args, **kwargs):
        super().save(*args, **kwargs)
    def get_absolute_url(self):
        return "/"
class PostTests(TestCase):
    def setUp(self):
        pass
    @classmethod
    def setUpTestData(cls):
        pass
    def tearDown(self):
        pass
Post, PostTests
